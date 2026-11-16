//! Device-scoped simulator tools shared by the GUI and agent bridge.
//! Consent and conversation/project checks are performed by the devtools router.
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, process::Stdio, sync::{Arc, OnceLock, Weak}, time::Duration};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::Command, sync::Mutex};

const MAX_IMAGE: usize = 8 * 1024 * 1024;
const MAX_TEXT: usize = 16 * 1024;
const INPUT_SOURCE: &str = include_str!("../../../scripts/mobile/ios-input.m");

struct Tools { xcrun: PathBuf, adb: PathBuf, emulator: PathBuf }
impl Tools {
    fn discover() -> Self {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        let mut roots: Vec<PathBuf> = ["ANDROID_HOME", "ANDROID_SDK_ROOT"].into_iter()
            .filter_map(std::env::var_os).map(PathBuf::from).collect();
        if let Some(home) = home { roots.push(home.join("Library/Android/sdk")); roots.push(home.join("Android/Sdk")); }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") { roots.push(PathBuf::from(local).join("Android/Sdk")); }
        let sdk_tool = |folder: &str, name: &str| {
            let filename = format!("{name}{}", if cfg!(windows) { ".exe" } else { "" });
            roots.iter().map(|root| root.join(folder).join(&filename)).find(|p| p.is_file())
                .unwrap_or_else(|| PathBuf::from(filename))
        };
        Self { xcrun: PathBuf::from("xcrun"), adb: sdk_tool("platform-tools", "adb"), emulator: sdk_tool("emulator", "emulator") }
    }
}

/// Commands never run through a host shell. Output and execution time are bounded.
async fn run(program: &Path, args: &[String], input: Option<&[u8]>, timeout: Duration, limit: usize) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(args).stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = command.spawn().with_context(|| format!("Could not start {}. Check that the required SDK is installed.", program.display()))?;
    let mut stdout = child.stdout.take().context("Missing command output")?;
    let mut stderr = child.stderr.take().context("Missing command diagnostics")?;
    let mut stdin = child.stdin.take();
    let operation = async {
        let write = async {
            if let (Some(pipe), Some(bytes)) = (stdin.as_mut(), input) { pipe.write_all(bytes).await?; pipe.shutdown().await?; }
            drop(stdin.take()); // ChildStdin shutdown alone does not close the pipe.
            Ok::<_, anyhow::Error>(())
        };
        let read_out = async { let mut bytes = Vec::new(); (&mut stdout).take((limit + 1) as u64).read_to_end(&mut bytes).await?; if bytes.len() > limit { bail!("Device tool output exceeded the size limit"); } Ok::<_, anyhow::Error>(bytes) };
        let read_err = async { let mut bytes = Vec::new(); (&mut stderr).take(64 * 1024).read_to_end(&mut bytes).await?; Ok::<_, anyhow::Error>(bytes) };
        let (_, out, err) = tokio::try_join!(write, read_out, read_err)?;
        if out.len() > limit { bail!("Device tool output exceeded the size limit"); }
        let status = child.wait().await?;
        if !status.success() {
            let message = String::from_utf8_lossy(&err);
            bail!("Device tool failed: {}", message.chars().take(1200).collect::<String>().trim());
        }
        Ok(out)
    };
    tokio::time::timeout(timeout, operation).await.context("Device operation timed out")?
}
fn strings(args: &[&str]) -> Vec<String> { args.iter().map(|s| (*s).to_owned()).collect() }
async fn text_command(program: &Path, args: &[&str]) -> Result<String> {
    let bytes = run(program, &strings(args), None, Duration::from_secs(20), 1024 * 1024).await?;
    Ok(String::from_utf8(bytes).context("Device tool returned non-UTF-8 metadata")?)
}
fn required<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    let value = params.get(key).and_then(Value::as_str).with_context(|| format!("Missing {key}"))?;
    if value.is_empty() || value.len() > MAX_TEXT || value.contains('\0') { bail!("Invalid {key}"); }
    Ok(value)
}
fn number(params: &Value, key: &str) -> Result<f64> {
    let value = params.get(key).and_then(Value::as_f64).with_context(|| format!("Missing {key}"))?;
    if !(0.0..=1.0).contains(&value) { bail!("{key} must be a normalized coordinate between 0 and 1"); }
    Ok(value)
}
fn gesture(params: &Value, swipe: bool) -> Result<Value> {
    let duration = params.get("duration_ms").map(|v| v.as_u64().context("Invalid duration_ms")).transpose()?.unwrap_or(if swipe { 350 } else { 180 });
    if !(100..=2000).contains(&duration) { bail!("duration_ms must be between 100 and 2000"); }
    let mut result = json!({"action":if swipe {"swipe"} else {"tap"},"x":number(params,"x")?,"y":number(params,"y")?,"duration_ms":duration});
    if swipe { result["x2"] = json!(number(params,"x2")?); result["y2"] = json!(number(params,"y2")?); }
    Ok(result)
}
fn image(bytes: Vec<u8>) -> Result<Value> {
    if bytes.len() < 24 || bytes.len() > MAX_IMAGE || bytes[..8] != *b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" { bail!("The device did not return a valid PNG screenshot"); }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0 || height == 0 || width > 16384 || height > 16384 { bail!("Invalid screenshot dimensions"); }
    Ok(json!({"mime_type":"image/png","data":STANDARD.encode(bytes),"width":width,"height":height}))
}
fn app_path(params: &Value, workspace: &Path, platform: &str) -> Result<PathBuf> {
    let path = Path::new(required(params, "app_path")?);
    let candidate = if path.is_absolute() { path.to_path_buf() } else { workspace.join(path) };
    let canonical = std::fs::canonicalize(candidate).context("App build does not exist")?;
    let root = std::fs::canonicalize(workspace)?;
    if !canonical.starts_with(root) { bail!("App build must be inside the attached project"); }
    if platform == "ios" && (!canonical.is_dir() || canonical.extension().is_none_or(|e| e != "app")) { bail!("Choose an iOS simulator .app directory"); }
    if platform == "android" && (!canonical.is_file() || canonical.extension().is_none_or(|e| e != "apk")) { bail!("Choose an Android .apk build"); }
    Ok(canonical)
}
fn bundle_id(params: &Value) -> Result<&str> {
    let id = required(params, "bundle_id")?;
    if id.len() > 255 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b,b'.'|b'_'|b'-')) || !id.contains('.') { bail!("Invalid app bundle/package identifier"); }
    Ok(id)
}
fn ios_devices(value: &Value) -> Vec<Value> {
    let mut result = Vec::new();
    if let Some(runtimes) = value.get("devices").and_then(Value::as_object) {
        for (runtime, devices) in runtimes {
            if !runtime.contains(".iOS-") { continue; }
            for d in devices.as_array().into_iter().flatten() {
                if d["isAvailable"].as_bool() != Some(true) { continue; }
                let Some(id) = d["udid"].as_str() else { continue; };
                if uuid::Uuid::parse_str(id).is_err() { continue; }
                result.push(json!({"id":id,"name":d["name"].as_str().unwrap_or(id),"platform":"ios","runtime":runtime.rsplit('.').next().unwrap_or(runtime),"state":if d["state"]=="Booted" {"booted"} else {"shutdown"}}));
            }
        }
    }
    result
}
async fn list_ios(tools: &Tools) -> Result<Vec<Value>> {
    let text = text_command(&tools.xcrun, &["simctl","list","devices","available","--json"]).await?;
    Ok(ios_devices(&serde_json::from_str(&text).context("Invalid simctl device list")?))
}
fn android_devices(text: &str) -> Vec<Value> {
    text.lines().filter_map(|line| {
        let mut columns = line.split_whitespace();
        let id = columns.next()?;
        if !valid_emulator_id(id) { return None; }
        let state = match columns.next()? { "device" => "booted", _ => "offline" };
        let name = columns.find_map(|s| s.strip_prefix("model:")).unwrap_or(id).replace('_', " ");
        Some(json!({"id":id,"name":name,"state":state,"platform":"android"}))
    }).collect()
}
fn valid_emulator_id(id: &str) -> bool {
    id.strip_prefix("emulator-").is_some_and(|port| !port.is_empty() && port.len() <= 5 && port.bytes().all(|b|b.is_ascii_digit()) && port.parse::<u16>().is_ok())
}
fn valid_avd(name: &str) -> bool { !name.is_empty() && !name.starts_with('-') && name.len() <= 128 && name.bytes().all(|b|b.is_ascii_alphanumeric() || matches!(b,b'_'|b'-'|b'.')) }
async fn list_android(tools: &Tools) -> Result<Vec<Value>> {
    let mut devices = android_devices(&text_command(&tools.adb, &["devices","-l"]).await?);
    let mut running = HashSet::new();
    for device in &mut devices {
        if device["state"] != "booted" { continue; }
        let id = device["id"].as_str().unwrap();
        if let Ok(name) = text_command(&tools.adb, &["-s",id,"emu","avd","name"]).await {
            let name = name.lines().find(|line| !line.is_empty() && *line != "OK").unwrap_or("");
            if valid_avd(name) { running.insert(name.to_owned()); device["avd_name"] = json!(name); device["name"] = json!(name); }
        }
    }
    if let Ok(avds) = text_command(&tools.emulator, &["-list-avds"]).await {
        for name in avds.lines().map(str::trim).filter(|name|valid_avd(name)) {
            if !running.contains(name) { devices.push(json!({"id":format!("avd:{name}"),"name":name,"avd_name":name,"state":"shutdown","platform":"android"})); }
        }
    }
    Ok(devices)
}
async fn ensure_android(tools: &Tools, id: &str) -> Result<()> {
    if !valid_emulator_id(id) { bail!("Choose a running Android emulator; physical devices are not supported"); }
    let value = text_command(&tools.adb, &["-s",id,"shell","getprop","ro.kernel.qemu"]).await?;
    if value.trim() != "1" { bail!("Selected device is not an Android emulator"); }
    Ok(())
}
fn quote_android(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }
async fn android_screenshot(tools: &Tools, id: &str) -> Result<Value> {
    image(run(&tools.adb,&strings(&["-s",id,"exec-out","screencap","-p"]),None,Duration::from_secs(20),MAX_IMAGE).await?)
}
async fn input_helper(tools: &Tools) -> Result<(PathBuf, String)> {
    static BUILD: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = BUILD.get_or_init(||Mutex::new(())).lock().await;
    let developer = std::env::var("DEVELOPER_DIR").ok().filter(|s| !s.is_empty()).unwrap_or(
        text_command(Path::new("xcode-select"), &["-p"]).await?.trim().to_owned());
    if !Path::new(&developer).is_absolute() { bail!("Xcode developer directory must be absolute"); }
    let root = crate::paths::data_root()?.join("mobile-helper");
    std::fs::create_dir_all(&root)?;
    let source = root.join("ios-input.m");
    let binary = root.join("ios-input");
    if binary.is_file() && std::fs::read_to_string(&source).ok().as_deref() == Some(INPUT_SOURCE) { return Ok((binary,developer)); }
    let temp_source = root.join(format!("ios-input-{}.m",uuid::Uuid::new_v4()));
    let temp_binary = temp_source.with_extension("bin");
    std::fs::write(&temp_source,INPUT_SOURCE)?;
    let args = vec!["clang".into(),"-fblocks".into(),"-framework".into(),"Foundation".into(),"-framework".into(),"AppKit".into(),temp_source.to_string_lossy().into_owned(),"-o".into(),temp_binary.to_string_lossy().into_owned()];
    let compilation = run(&tools.xcrun,&args,None,Duration::from_secs(60),1024*1024).await;
    let _ = std::fs::remove_file(&temp_source);
    compilation.context("Could not build the iOS input helper with the selected Xcode")?;
    std::fs::rename(&temp_binary,&binary)?;
    std::fs::write(&source,INPUT_SOURCE)?;
    Ok((binary,developer))
}
async fn ios_screenshot(tools: &Tools, id: &str) -> Result<Value> {
    ios_screenshot_in(tools, id, &crate::paths::data_root()?).await
}
async fn ios_screenshot_in(tools: &Tools, id: &str, root: &Path) -> Result<Value> {
    // simctl treats "-" as a filename on current Xcode, not stdout. Capture in
    // the private engine directory and remove the artifact on every outcome.
    let path = root.join(format!("mobile-frame-{}.png", uuid::Uuid::new_v4()));
    let capture = async {
        run(&tools.xcrun, &strings(&["simctl", "io", id, "screenshot", "--type=png", &path.to_string_lossy()]), None, Duration::from_secs(20), 64 * 1024).await?;
        let file = tokio::fs::File::open(&path).await.context("Simulator screenshot was not created")?;
        let mut bytes = Vec::new();
        file.take((MAX_IMAGE + 1) as u64).read_to_end(&mut bytes).await?;
        image(bytes)
    }.await;
    let _ = tokio::fs::remove_file(&path).await;
    capture
}
async fn ios_input(tools: &Tools, id: &str, request: Value) -> Result<Value> {
    let (helper, developer) = input_helper(tools).await?;
    let input = serde_json::to_vec(&request)?;
    let output = run(&helper,&[developer,id.to_owned()],Some(&input),Duration::from_secs(15),64*1024).await?;
    serde_json::from_slice(&output).context("Invalid iOS input helper response")
}

pub(crate) async fn dispatch(action: &str, params: Value, workspace: &Path) -> Result<Value> {
    // Serialize actions on a device across conversations so a paste or gesture
    // cannot interleave with another controller's input. Metadata remains parallel.
    static DEVICES: OnceLock<std::sync::Mutex<HashMap<String, Weak<Mutex<()>>>>> = OnceLock::new();
    let device_lock = if action != "list" && action != "status" {
        let key = format!("{}:{}", required(&params, "platform")?, required(&params, "device_id")?.to_lowercase());
        let mut locks = DEVICES.get_or_init(Default::default).lock().map_err(|_| anyhow::anyhow!("Device lock unavailable"))?;
        locks.retain(|_, value| value.strong_count() > 0);
        let lock = locks.get(&key).and_then(Weak::upgrade).unwrap_or_else(|| Arc::new(Mutex::new(())));
        locks.insert(key, Arc::downgrade(&lock));
        Some(lock)
    } else { None };
    let _guard = match device_lock.as_ref() { Some(lock) => Some(lock.lock().await), None => None };
    dispatch_with(&Tools::discover(), action, params, workspace).await
}
async fn dispatch_with(tools: &Tools, action: &str, params: Value, workspace: &Path) -> Result<Value> {
    let platform = required(&params,"platform")?;
    if platform != "ios" && platform != "android" { bail!("platform must be ios or android"); }
    if action == "list" || action == "status" {
        let found = if platform == "ios" {
            if !cfg!(target_os="macos") { return Ok(json!({"available":false,"reason":"iOS Simulator requires macOS and Xcode.","devices":[],"capabilities":{}})); }
            list_ios(tools).await
        } else { list_android(tools).await };
        return Ok(match found {
            Ok(devices) => json!({"available":true,"devices":devices,"capabilities":{"touch":true,"keyboard":true,"screenshot":true,"install":true,"launch":true},"notes":if platform=="ios" {Vec::<&str>::new()} else {vec!["Android text injection supports ASCII. Use the emulator keyboard for other scripts."]}}),
            Err(error) => json!({"available":false,"reason":error.to_string(),"devices":[],"capabilities":{}}),
        });
    }
    if !["boot","shutdown","screenshot","tap","swipe","type","key","install","launch"].contains(&action) { bail!("Unknown mobile action"); }
    let id = required(&params,"device_id")?;
    if platform == "ios" {
        if !cfg!(target_os="macos") { bail!("iOS Simulator requires macOS and Xcode"); }
        uuid::Uuid::parse_str(id).context("Invalid simulator UDID")?;
        let devices = list_ios(tools).await?;
        let device = devices.iter().find(|d| d["id"].as_str().is_some_and(|v|v.eq_ignore_ascii_case(id))).context("Selected iOS simulator is unavailable")?;
        if action == "boot" {
            if device["state"] != "booted" { text_command(&tools.xcrun,&["simctl","boot",id]).await?; }
            run(&tools.xcrun,&strings(&["simctl","bootstatus",id,"-b"]),None,Duration::from_secs(120),1024*1024).await?;
            return Ok(json!({"ok":true,"device_id":id,"state":"booted"}));
        }
        if device["state"] != "booted" { bail!("Boot the selected simulator first"); }
        match action {
            "shutdown" => { text_command(&tools.xcrun,&["simctl","shutdown",id]).await?; }
            "screenshot" => return ios_screenshot(tools,id).await,
            "tap"|"swipe" => return ios_input(tools,id,gesture(&params,action=="swipe")?).await,
            "type" => {
                let text = required(&params,"text")?;
                if text.len() > 256 || !text.is_ascii() || text.chars().any(|c| c.is_ascii_control() && c != '\n' && c != '\t') {
                    bail!("iOS text input supports up to 256 US ASCII characters. Use the device's on-screen keyboard for other scripts.");
                }
                return ios_input(tools,id,json!({"action":"type","text":text})).await;
            }
            "key" => {
                let key = required(&params,"key")?;
                if !["home","lock","volume_up","volume_down","enter","backspace","tab","escape"].contains(&key) { bail!("Unsupported iOS key"); }
                return ios_input(tools,id,json!({"action":"key","key":key})).await;
            }
            "install" => {
                let path = app_path(&params,workspace,platform)?;
                run(&tools.xcrun,&strings(&["simctl","install",id,&path.to_string_lossy()]),None,Duration::from_secs(90),1024*1024).await?;
            }
            "launch" => { text_command(&tools.xcrun,&["simctl","launch",id,bundle_id(&params)?]).await?; }
            _ => unreachable!(),
        }
    } else {
        if action == "boot" { return boot_android(tools,id).await; }
        ensure_android(tools,id).await?;
        match action {
            "shutdown" => { text_command(&tools.adb,&["-s",id,"emu","kill"]).await?; }
            "screenshot" => return android_screenshot(tools,id).await,
            "tap"|"swipe" => {
                let g = gesture(&params,action=="swipe")?;
                let frame = android_screenshot(tools,id).await?;
                let coord = |key:&str, dimension:&str| ((g[key].as_f64().unwrap() * (frame[dimension].as_u64().unwrap()-1) as f64).round() as u64).to_string();
                let mut args = strings(&["-s",id,"shell","input",action]);
                args.extend([coord("x","width"),coord("y","height")]);
                if action == "swipe" { args.extend([coord("x2","width"),coord("y2","height"),g["duration_ms"].as_u64().unwrap().to_string()]); }
                run(&tools.adb,&args,None,Duration::from_secs(15),64*1024).await?;
            }
            "type" => {
                let text = required(&params,"text")?;
                if !text.is_ascii() || text.contains(['\n','\r']) || text.contains("%s") { bail!("Android SDK text input supports single-line ASCII without literal %s. Use the emulator keyboard for this text."); }
                let escaped = quote_android(&text.replace(' ',"%s"));
                text_command(&tools.adb,&["-s",id,"shell","input","text",&escaped]).await?;
            }
            "key" => {
                let code = match required(&params,"key")? {"home"=>"3","back"=>"4","enter"=>"66","backspace"=>"67","tab"=>"61","escape"=>"111","lock"=>"26","volume_up"=>"24","volume_down"=>"25",_=>bail!("Unsupported Android key")};
                text_command(&tools.adb,&["-s",id,"shell","input","keyevent",code]).await?;
            }
            "install" => {
                let path=app_path(&params,workspace,platform)?;
                run(&tools.adb,&strings(&["-s",id,"install","-r",&path.to_string_lossy()]),None,Duration::from_secs(90),1024*1024).await?;
            }
            "launch" => {
                let package=bundle_id(&params)?;
                let resolved=text_command(&tools.adb,&["-s",id,"shell","cmd","package","resolve-activity","--brief","-a","android.intent.action.MAIN","-c","android.intent.category.LAUNCHER","-p",package]).await?;
                let component=resolved.lines().rev().map(str::trim).find(|line| line.contains('/')).context("This Android package has no launcher activity")?;
                if !component.starts_with(&format!("{package}/")) || !component.bytes().all(|c| c.is_ascii_alphanumeric() || b"._/$".contains(&c)) { bail!("Android returned an invalid launcher activity"); }
                let output=text_command(&tools.adb,&["-s",id,"shell","am","start","-W","-n",&quote_android(component)]).await?;
                if output.lines().any(|line| line.starts_with("Error:")) { bail!("Android launch failed: {}",output.trim()); }
            }
            _ => unreachable!(),
        }
    }
    Ok(json!({"ok":true,"device_id":id}))
}
async fn boot_android(tools:&Tools,id:&str)->Result<Value> {
    if valid_emulator_id(id) { ensure_android(tools,id).await?; return Ok(json!({"ok":true,"device_id":id,"state":"booted"})); }
    let name = id.strip_prefix("avd:").filter(|name|valid_avd(name)).context("Choose a valid Android virtual device")?;
    let devices=list_android(tools).await?;
    if let Some(existing) = devices.iter().find(|d|d["avd_name"]==name && d["state"]=="booted") { return Ok(json!({"ok":true,"device_id":existing["id"],"state":"booted"})); }
    if !devices.iter().any(|d| d["id"]==id) { bail!("The selected Android virtual device no longer exists"); }
    let occupied:HashSet<_>=devices.iter().filter_map(|d|d["id"].as_str()).collect();
    let port=(5554u16..5682).step_by(2).find(|port| !occupied.contains(format!("emulator-{port}").as_str()) && std::net::TcpListener::bind(("127.0.0.1",*port)).is_ok() && std::net::TcpListener::bind(("127.0.0.1",*port+1)).is_ok()).context("No free Android emulator port")?;
    let port_text=port.to_string();
    let mut child=Command::new(&tools.emulator).args(["-avd",name,"-port",&port_text,"-no-snapshot-save"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(false).spawn().context("Could not start Android Emulator")?;
    let serial=format!("emulator-{port}");
    let ready=async {
        for _ in 0..90 {
            if let Some(status)=child.try_wait()? { bail!("Android Emulator exited before booting ({status}). Open its AVD in Android Studio to diagnose it."); }
            if let Ok(value)=text_command(&tools.adb,&["-s",&serial,"shell","getprop","sys.boot_completed"]).await {
                if value.trim()=="1" { return Ok::<(),anyhow::Error>(()); }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        bail!("Android Emulator did not finish booting");
    };
    let boot_result=tokio::time::timeout(Duration::from_secs(120),ready).await.context("Android Emulator boot timed out").and_then(|result|result);
    if let Err(error)=boot_result { let _=child.kill().await; return Err(error); }
    // Keep the virtual device available after the pane is hidden; no external device is stopped.
    tokio::spawn(async move { let _=child.wait().await; });
    Ok(json!({"ok":true,"device_id":serial,"state":"booted"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_unavailable_and_non_ios_simulators() {
        let devices=ios_devices(&json!({"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-18-0":[{"udid":"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE","name":"iPhone","state":"Booted","isAvailable":true},{"udid":"11111111-2222-3333-4444-555555555555","isAvailable":false}],"com.apple.CoreSimulator.SimRuntime.tvOS-18-0":[{"udid":"11111111-2222-3333-4444-555555555555","isAvailable":true}]}}));
        assert_eq!(devices.len(),1); assert_eq!(devices[0]["state"],"booted");
    }
    #[test]
    fn never_lists_physical_android_devices() {
        let devices=android_devices("List of devices attached\nreal-phone device model:Pixel_9\nemulator-5554 device product:sdk model:Virtual_Pixel\nemulator-nope device\nemulator-5556 offline\n");
        assert_eq!(devices.len(),2); assert_eq!(devices[0]["name"],"Virtual Pixel"); assert_eq!(devices[1]["state"],"offline");
        assert!(!valid_emulator_id("emulator-5554;id")); assert!(!valid_emulator_id("emulator-999999"));
    }
    #[test]
    fn rejects_outside_coordinates_and_bad_duration() {
        assert!(gesture(&json!({"x":1.1,"y":0}),false).is_err());
        assert!(gesture(&json!({"x":0,"y":0,"duration_ms":5000}),false).is_err());
        assert!(gesture(&json!({"x":0,"y":0}),true).is_err());
        assert_eq!(gesture(&json!({"x":0.5,"y":0.5}),false).unwrap()["duration_ms"],180);
    }
    #[test]
    fn validates_screenshot_and_shell_arguments() {
        assert!(image(vec![0;24]).is_err());
        assert_eq!(quote_android("a'$(touch x);b"),"'a'\\''$(touch x);b'");
        assert!(bundle_id(&json!({"bundle_id":"com.example;id"})).is_err());
        assert!(!valid_avd("-wipe-data")); // leading options are not valid AVD names
    }
    #[test]
    fn project_build_boundary_and_symlinks() {
        let dir=std::env::temp_dir().join(format!("studio-mobile-test-{}",uuid::Uuid::new_v4()));
        let project=dir.join("project"); std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("app.apk"),b"fixture").unwrap();
        std::fs::write(dir.join("outside.apk"),b"fixture").unwrap();
        assert!(app_path(&json!({"app_path":"app.apk"}),&project,"android").is_ok());
        assert!(app_path(&json!({"app_path":"../outside.apk"}),&project,"android").is_err());
        #[cfg(unix)] { std::os::unix::fs::symlink(dir.join("outside.apk"),project.join("link.apk")).unwrap(); assert!(app_path(&json!({"app_path":"link.apk"}),&project,"android").is_err()); }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn fake_android_sdk_dispatches_frames_and_rejects_physical_devices() {
        use std::os::unix::fs::PermissionsExt;
        let dir=std::env::temp_dir().join(format!("studio-mobile-sdk-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script=dir.join("sdk-tool");
        let log=dir.join("args.log");
        let png=dir.join("frame.png");
        let mut frame=b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        frame.extend_from_slice(&320u32.to_be_bytes()); frame.extend_from_slice(&640u32.to_be_bytes());
        std::fs::write(&png,frame).unwrap();
        std::fs::write(&script,format!(r#"#!/bin/sh
export HOME='{root}' CODEX_HOME='{root}/codex' CLAUDE_CONFIG_DIR='{root}/claude' AGENTKIT_DATA_DIR='{root}/state'
printf '%s\n' "$*" >> '{log}'
case "$*" in
'devices -l') printf 'List of devices attached\nphysical-device device model:Phone\nemulator-5554 device model:Pixel\n' ;;
'-list-avds') printf 'Pixel\nTablet\n' ;;
'-s emulator-5554 emu avd name') printf 'Pixel\nOK\n' ;;
'-s emulator-5554 shell getprop ro.kernel.qemu') printf '1\n' ;;
'-s emulator-5554 exec-out screencap -p') cat '{png}' ;;
'-s emulator-5554 shell input tap 160 320') exit 0 ;;
*) echo 'Unexpected SDK arguments' >&2; exit 2 ;;
esac
"#,root=dir.display(),log=log.display(),png=png.display())).unwrap();
        std::fs::set_permissions(&script,std::fs::Permissions::from_mode(0o700)).unwrap();
        let tools=Tools{xcrun:script.clone(),adb:script.clone(),emulator:script};
        let listed=dispatch_with(&tools,"list",json!({"platform":"android"}),&dir).await.unwrap();
        assert_eq!(listed["available"],true);
        assert_eq!(listed["devices"].as_array().unwrap().len(),2);
        assert_eq!(listed["devices"][1]["id"],"avd:Tablet");
        let frame=dispatch_with(&tools,"screenshot",json!({"platform":"android","device_id":"emulator-5554"}),&dir).await.unwrap();
        assert_eq!(frame["width"],320); assert_eq!(frame["height"],640);
        dispatch_with(&tools,"tap",json!({"platform":"android","device_id":"emulator-5554","x":0.5,"y":0.5}),&dir).await.unwrap();
        let before=std::fs::read_to_string(&log).unwrap();
        assert!(dispatch_with(&tools,"screenshot",json!({"platform":"android","device_id":"physical-device"}),&dir).await.is_err());
        assert_eq!(before,std::fs::read_to_string(&log).unwrap());
        assert!(dispatch_with(&tools,"type",json!({"platform":"android","device_id":"emulator-5554","text":"سلام"}),&dir).await.unwrap_err().to_string().contains("ASCII"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn command_output_is_bounded() {
        let error=run(Path::new("/usr/bin/printf"),&strings(&["123456789"]),None,Duration::from_secs(2),3).await.unwrap_err();
        assert!(error.to_string().contains("size limit"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn simctl_screenshot_reads_file_and_removes_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir=std::env::temp_dir().join(format!("studio-simctl-frame-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source=dir.join("fixture.png"); let script=dir.join("xcrun");
        let mut bytes=b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&1206u32.to_be_bytes());bytes.extend_from_slice(&2622u32.to_be_bytes());
        std::fs::write(&source,bytes).unwrap();
        std::fs::write(&script,format!("#!/bin/sh\nexport HOME='{0}' CODEX_HOME='{0}/codex' CLAUDE_CONFIG_DIR='{0}/claude' AGENTKIT_DATA_DIR='{0}/state'\ncp '{1}' \"$6\"\nprintf 'Wrote screenshot to file\\n'\n",dir.display(),source.display())).unwrap();
        std::fs::set_permissions(&script,std::fs::Permissions::from_mode(0o700)).unwrap();
        let tools=Tools{xcrun:script.clone(),adb:script.clone(),emulator:script};
        let frame=ios_screenshot_in(&tools,"AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE",&dir).await.unwrap();
        assert_eq!(frame["width"],1206);assert_eq!(frame["height"],2622);
        assert!(!std::fs::read_dir(&dir).unwrap().any(|f|f.unwrap().file_name().to_string_lossy().starts_with("mobile-frame-")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_input_closes_before_waiting_for_output() {
        let output=run(Path::new("/bin/cat"),&[],Some(b"synthetic input"),Duration::from_secs(2),128).await.unwrap();
        assert_eq!(output,b"synthetic input");
    }

}
