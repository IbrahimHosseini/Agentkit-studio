/*
MIT License (referenced simulator HID ABI portions)

Copyright (c) Meta Platforms, Inc. and affiliates.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/
// Agentkit Studio's device-scoped input bridge. No host mouse/keyboard events.
// SimulatorKit is private API: every required class/selector/symbol is checked.
// ABI references (MIT attribution in scripts/mobile/NOTICE.md): Meta idb PrivateHeaders/SimulatorApp/Indigo.h and
// FBSimulatorControl/HID/SimulatorIndigoHID.swift (MIT), documented in docs/devtools-mobile.md.
#import <Foundation/Foundation.h>
#import <AppKit/AppKit.h>
#import <objc/message.h>
#import <dlfcn.h>
#import <malloc/malloc.h>
#import <xpc/xpc.h>

@interface NSObject (StudioSimulator)
+ (id)sharedServiceContextForDeveloperDir:(NSString *)path error:(NSError **)error;
- (id)defaultDeviceSetWithError:(NSError **)error;
- (NSArray *)devices;
- (NSUUID *)UDID;
- (mach_port_t)lookup:(NSString *)service error:(NSError **)error;
- (id)initWithDevice:(id)device error:(NSError **)error;
- (void)sendWithMessage:(void *)message freeWhenDone:(BOOL)release
      completionQueue:(dispatch_queue_t)queue completion:(void (^)(NSError *))completion;
@end

typedef void *(*MouseMessage)(CGPoint *, CGPoint *, uint32_t, NSUInteger, CGSize, uint32_t);
typedef void *(*KeyMessage)(uint32_t, uint32_t);
typedef void *(*ButtonMessage)(uint32_t, uint32_t, uint32_t);
typedef void *(*UsageMessage)(uint32_t, uint32_t, uint32_t, uint32_t);

static void fail(NSString *message) {
  fprintf(stderr, "%s\n", message.UTF8String);
  exit(1);
}
// Xcode 27 routes guest HID through dtuhidd instead of the legacy Indigo port.
// The envelope/endpoint ABI is documented by Meta idb's DTUHIDModels.swift and
// SimulatorXPCConnection.swift (same MIT attribution as the legacy ABI above).
static xpc_connection_t modern = NULL;
static const char *hidService = "com.apple.coredevice.feature.remote.hid.digitizer";
static BOOL contactDown = NO;
static xpc_object_t envelope(const char *type, xpc_object_t payload, BOOL barrier) {
  xpc_object_t message = xpc_dictionary_create(NULL, NULL, 0);
  xpc_dictionary_set_string(message, "messageType", type);
  xpc_dictionary_set_string(message, "featureIdentifier", hidService);
  xpc_dictionary_set_bool(message, "isBarrier", barrier);
  xpc_dictionary_set_value(message, "payload", payload);
  return message;
}
static void modernSend(const char *type, xpc_object_t payload) {
  xpc_object_t message = envelope(type, payload, NO);
  dispatch_semaphore_t sent = dispatch_semaphore_create(0);
  xpc_connection_send_message(modern, message);
  xpc_connection_send_barrier(modern, ^{ dispatch_semaphore_signal(sent); });
  if (dispatch_semaphore_wait(sent, dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC))) fail(@"Simulator HID send timed out.");
  xpc_release(message); xpc_release(payload);
}
static void connectModern(id device) {
  if (![device respondsToSelector:@selector(lookup:error:)]) return;
  typedef xpc_object_t (*Endpoint)(mach_port_t, uint64_t, uint64_t);
  typedef void (*Enable)(xpc_connection_t);
  Endpoint endpointForPort = (Endpoint)dlsym(RTLD_DEFAULT, "xpc_endpoint_create_mach_port_4sim");
  Enable enable = (Enable)dlsym(RTLD_DEFAULT, "xpc_connection_enable_sim2host_4sim");
  if (!endpointForPort || !enable) return;
  NSError *error = nil;
  mach_port_t port = [device lookup:@(hidService) error:&error];
  if (!port) return; // Older runtimes do not vend this service.
  xpc_object_t endpoint = endpointForPort(port, 0, 0);
  if (!endpoint) fail(@"Cannot create the simulator HID endpoint.");
  modern = xpc_connection_create_from_endpoint(endpoint);
  xpc_release(endpoint);
  if (!modern) fail(@"Cannot connect to simulator HID.");
  enable(modern);
  xpc_connection_set_target_queue(modern, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0));
  xpc_connection_set_event_handler(modern, ^(xpc_object_t event) { (void)event; });
  xpc_connection_resume(modern);
  xpc_object_t payload = xpc_dictionary_create(NULL, NULL, 0);
  xpc_dictionary_set_uint64(payload, "usageCode", 0);
  xpc_dictionary_set_uint64(payload, "state", 2);
  xpc_object_t message = envelope("IndigoKeyboardButtonEvent", payload, YES);
  dispatch_semaphore_t live = dispatch_semaphore_create(0);
  __block BOOL replied = NO;
  xpc_connection_send_message_with_reply(modern, message, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(xpc_object_t response) {
    replied = xpc_get_type(response) != XPC_TYPE_ERROR;
    dispatch_semaphore_signal(live);
  });
  if (dispatch_semaphore_wait(live, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) || !replied)
    fail(@"The simulator HID service did not respond. Wait for boot to finish and try again.");
  xpc_release(message); xpc_release(payload);
  [NSThread sleepForTimeInterval:0.2];
}
static void modernKey(uint64_t code, uint64_t page, BOOL hardware, BOOL up) {
  xpc_object_t payload = xpc_dictionary_create(NULL, NULL, 0);
  xpc_dictionary_set_uint64(payload, "usageCode", code);
  xpc_dictionary_set_uint64(payload, "state", up ? 2 : 1);
  if (hardware) xpc_dictionary_set_uint64(payload, "usagePage", page);
  modernSend(hardware ? "IndigoButtonEvent" : "IndigoKeyboardButtonEvent", payload);
}
static void sendMessage(id client, void *message) {
  if (!message) fail(@"SimulatorKit did not create an input event for this Xcode version.");
  dispatch_semaphore_t signal = dispatch_semaphore_create(0);
  __block NSError *failure = nil;
  [client sendWithMessage:message freeWhenDone:YES completionQueue:dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0)
    completion:^(NSError *error) { failure = [error retain]; dispatch_semaphore_signal(signal); }];
  if (dispatch_semaphore_wait(signal, dispatch_time(DISPATCH_TIME_NOW, 3 * NSEC_PER_SEC)))
    fail(@"Simulator input delivery timed out.");
  if (failure) fail(failure.localizedDescription);
}
static void touch(id client, MouseMessage create, double x, double y, BOOL up) {
  if (modern) {
    xpc_object_t point = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_double(point, "x", x); xpc_dictionary_set_double(point, "y", y);
    xpc_object_t payload = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_value(payload, "pointOne", point);
    xpc_dictionary_set_uint64(payload, "eventType", up ? 2 : contactDown ? 1 : 0);
    xpc_dictionary_set_uint64(payload, "edge", 0); xpc_dictionary_set_uint64(payload, "target", 0);
    contactDown = !up;
    xpc_release(point); modernSend("IndigoDigitizerEvent", payload);
    return;
  }
  CGPoint point = CGPointMake(x, y);
  void *message = create(&point, NULL, 0x32, up ? NSEventTypeLeftMouseUp : NSEventTypeLeftMouseDown, CGSizeMake(1, 1), 0);
  // The factory creates a multi-contact envelope, even with a null second
  // point. Re-envelope its digitizer payload for one contact. The private
  // wire layout is guarded before any reads (see the idb ABI references).
  if (!message || malloc_size(message) < 0xb0) {
    if (message) free(message);
    fail(@"This Xcode returned an unsupported touch message layout.");
  }
  uint8_t *single = calloc(1, 0x140);
  if (!single) { free(message); fail(@"Cannot allocate simulator touch event."); }
  uint32_t payloadBytes = 0x90;
  memcpy(single + 0x18, &payloadBytes, sizeof(payloadBytes));
  single[0x1c] = 2;
  memcpy(single + 0x20, (uint8_t *)message + 0x20, payloadBytes);
  memcpy(single + 0x3c, &x, sizeof(x));
  memcpy(single + 0x44, &y, sizeof(y));
  memcpy(single + 0xb0, single + 0x20, payloadBytes);
  uint32_t identity = 1, contact = 2;
  memcpy(single + 0xc0, &identity, sizeof(identity));
  memcpy(single + 0xc4, &contact, sizeof(contact));
  free(message);
  message = single;
  sendMessage(client, message);
}
static void keyboardEvent(id client, KeyMessage keyboard, uint32_t code, BOOL up) {
  if (modern) modernKey(code, 0, NO, up);
  else { if (!keyboard) fail(@"Keyboard input is unavailable in this Xcode."); sendMessage(client, keyboard(code, up ? 2 : 1)); }
}
static BOOL characterKey(unichar c, uint32_t *key, BOOL *shift) {
  *shift = NO;
  if (c >= 'a' && c <= 'z') { *key = 4 + c - 'a'; return YES; }
  if (c >= 'A' && c <= 'Z') { *key = 4 + c - 'A'; *shift = YES; return YES; }
  NSString *digits = @"1234567890", *shiftDigits = @"!@#$%^&*()";
  for (NSUInteger i = 0; i < digits.length; i++) {
    if (c == [digits characterAtIndex:i] || c == [shiftDigits characterAtIndex:i]) { *key = 30 + (uint32_t)i; *shift = c == [shiftDigits characterAtIndex:i]; return YES; }
  }
  NSString *plain = @" -=[]\\;'`,./", *upper = @" _+{}|:\"~<>?";
  uint32_t codes[] = {44,45,46,47,48,49,51,52,53,54,55,56};
  for (NSUInteger i = 0; i < plain.length; i++) {
    if (c == [plain characterAtIndex:i] || c == [upper characterAtIndex:i]) { *key = codes[i]; *shift = i && c == [upper characterAtIndex:i]; return YES; }
  }
  if (c == '\n') { *key = 40; return YES; }
  if (c == '\t') { *key = 43; return YES; }
  return NO;
}
static void sendTextInput(id client, KeyMessage keyboard, NSString *text) {
  if (![text isKindOfClass:NSString.class] || text.length == 0 || text.length > 256) fail(@"Enter 1 to 256 characters per input action.");
  uint32_t key = 0; BOOL shift = NO;
  for (NSUInteger i = 0; i < text.length; i++)
    if (!characterKey([text characterAtIndex:i], &key, &shift)) fail(@"Simulator text input currently supports the US ASCII keyboard. Use the device's on-screen keyboard for other scripts.");
  for (NSUInteger i = 0; i < text.length; i++) {
    characterKey([text characterAtIndex:i], &key, &shift);
    if (shift) { keyboardEvent(client, keyboard, 0xe1, NO); [NSThread sleepForTimeInterval:0.01]; }
    keyboardEvent(client, keyboard, key, NO); [NSThread sleepForTimeInterval:0.02];
    keyboardEvent(client, keyboard, key, YES);
    if (shift) keyboardEvent(client, keyboard, 0xe1, YES);
    [NSThread sleepForTimeInterval:0.01];
  }
}
static double coordinate(NSDictionary *request, NSString *key) {
  id value = request[key];
  if (![value isKindOfClass:NSNumber.class] || !isfinite([value doubleValue]) || [value doubleValue] < 0 || [value doubleValue] > 1)
    fail(@"Touch coordinates must be numbers from 0 to 1.");
  return [value doubleValue];
}
int main(int argc, const char *argv[]) {
  @autoreleasepool {
    if (argc != 3) fail(@"Expected the selected Xcode developer directory and simulator UDID.");
    NSString *developer = @(argv[1]);
    NSUUID *uuid = [[[NSUUID alloc] initWithUUIDString:@(argv[2])] autorelease];
    if (!uuid) fail(@"Invalid simulator UDID.");
    NSData *input = [[NSFileHandle fileHandleWithStandardInput] readDataToEndOfFile];
    if (input.length > 32768) fail(@"Input request is too large.");
    NSDictionary *request = [NSJSONSerialization JSONObjectWithData:input options:0 error:nil];
    if (![request isKindOfClass:NSDictionary.class]) fail(@"Invalid input JSON.");
    NSString *action = request[@"action"];
    if (![@[@"tap", @"swipe", @"key", @"type"] containsObject:action]) fail(@"Unknown input action.");
    NSString *core = @"/Library/Developer/PrivateFrameworks/CoreSimulator.framework/CoreSimulator";
    if (!dlopen(core.UTF8String, RTLD_NOW | RTLD_GLOBAL)) fail(@"CoreSimulator framework is unavailable. Install Xcode.");
    NSString *kit = [[developer stringByDeletingLastPathComponent] stringByAppendingPathComponent:@"SharedFrameworks/SimulatorKit.framework/SimulatorKit"];
    void *library = dlopen(kit.UTF8String, RTLD_NOW | RTLD_GLOBAL);
    if (!library) fail(@"SimulatorKit could not load from the selected Xcode. Check xcode-select.");
    Class contextClass = NSClassFromString(@"SimServiceContext");
    if (![contextClass respondsToSelector:@selector(sharedServiceContextForDeveloperDir:error:)]) fail(@"Unsupported CoreSimulator service API.");
    NSError *error = nil;
    id context = [contextClass sharedServiceContextForDeveloperDir:developer error:&error];
    if (!context || ![context respondsToSelector:@selector(defaultDeviceSetWithError:)]) fail(error.localizedDescription ?: @"Simulator service is unavailable.");
    id set = [context defaultDeviceSetWithError:&error];
    if (!set || ![set respondsToSelector:@selector(devices)]) fail(error.localizedDescription ?: @"Simulator device set is unavailable.");
    id device = nil;
    for (id candidate in [set devices]) {
      if ([candidate respondsToSelector:@selector(UDID)] && [[candidate UDID] isEqual:uuid]) { device = candidate; break; }
    }
    if (!device) fail(@"The selected simulator no longer exists.");
    connectModern(device);
    Class hidClass = NSClassFromString(@"_TtC12SimulatorKit24SimDeviceLegacyHIDClient");
    if (!modern && (!hidClass || ![hidClass instancesRespondToSelector:@selector(initWithDevice:error:)] ||
        ![hidClass instancesRespondToSelector:@selector(sendWithMessage:freeWhenDone:completionQueue:completion:)]))
      fail(@"This Xcode does not expose the supported SimulatorKit HID transport.");
    id client = modern ? nil : [[hidClass alloc] initWithDevice:device error:&error];
    if (!modern && !client) fail(error.localizedDescription ?: @"Cannot connect input to this simulator. Boot it first.");
    MouseMessage mouse = (MouseMessage)dlsym(library, "IndigoHIDMessageForMouseNSEvent");
    KeyMessage keyboard = (KeyMessage)dlsym(library, "IndigoHIDMessageForKeyboardArbitrary");
    ButtonMessage button = (ButtonMessage)dlsym(library, "IndigoHIDMessageForButton");
    UsageMessage usage = (UsageMessage)dlsym(library, "IndigoHIDMessageForHIDArbitrary");
    if ([action isEqual:@"type"]) {
      sendTextInput(client, keyboard, request[@"text"]);
    } else if ([action isEqual:@"tap"] || [action isEqual:@"swipe"]) {
      if (!modern && !mouse) fail(@"This Xcode does not support touch injection.");
      double x = coordinate(request, @"x"), y = coordinate(request, @"y");
      double x2 = x, y2 = y;
      if ([action isEqual:@"swipe"]) { x2 = coordinate(request, @"x2"); y2 = coordinate(request, @"y2"); }
      NSUInteger duration = request[@"duration_ms"] ? [request[@"duration_ms"] unsignedIntegerValue] : 180;
      if (duration < 100 || duration > 2000) fail(@"Gesture duration must be between 100 and 2000 milliseconds.");
      touch(client, mouse, x, y, NO);
      NSUInteger steps = [action isEqual:@"tap"] ? 1 : 20;
      for (NSUInteger step = 1; step <= steps; step++) {
        [NSThread sleepForTimeInterval:(double)duration / 1000 / steps];
        if (steps > 1) touch(client, mouse, x + (x2 - x) * step / steps, y + (y2 - y) * step / steps, NO);
      }
      touch(client, mouse, x2, y2, YES);
    } else {
      NSString *key = request[@"key"];
      if (modern) {
        NSDictionary *hardware = @{@"home":@0x40, @"lock":@0x30, @"volume_up":@0xe9, @"volume_down":@0xea};
        NSDictionary *keys = @{@"enter":@40, @"backspace":@42, @"tab":@43, @"escape":@41, @"paste":@25};
        NSNumber *code = hardware[key] ?: keys[key];
        if (!code) fail(@"Unsupported iOS key.");
        BOOL paste = [key isEqual:@"paste"];
        if (paste) { modernKey(0xe3, 0, NO, NO); [NSThread sleepForTimeInterval:0.08]; }
        modernKey(code.unsignedLongLongValue, 0x0c, hardware[key] != nil, NO);
        [NSThread sleepForTimeInterval:0.08];
        modernKey(code.unsignedLongLongValue, 0x0c, hardware[key] != nil, YES);
        if (paste) { [NSThread sleepForTimeInterval:0.08]; modernKey(0xe3, 0, NO, YES); }
        [NSThread sleepForTimeInterval:0.15];
        puts("{\"ok\":true}");
        return 0;
      }
      if ([key isEqual:@"home"] || [key isEqual:@"lock"]) {
        if (!button) fail(@"Hardware buttons are unavailable in this Xcode.");
        uint32_t code = [key isEqual:@"home"] ? 0 : 1;
        sendMessage(client, button(code, 1, 0x33)); sendMessage(client, button(code, 2, 0x33));
      } else if ([key isEqual:@"volume_up"] || [key isEqual:@"volume_down"]) {
        if (!usage) fail(@"Volume input is unavailable in this Xcode.");
        uint32_t code = [key isEqual:@"volume_up"] ? 0xe9 : 0xea;
        sendMessage(client, usage(0x32, 0x0c, code, 1)); sendMessage(client, usage(0x32, 0x0c, code, 2));
      } else {
        if (!keyboard) fail(@"Keyboard input is unavailable in this Xcode.");
        NSDictionary *keys = @{@"enter":@40, @"backspace":@42, @"tab":@43, @"escape":@41, @"paste":@25};
        NSNumber *code = keys[key];
        if (!code) fail(@"Unsupported iOS key.");
        BOOL paste = [key isEqual:@"paste"];
        if (paste) sendMessage(client, keyboard(0xe3, 1));
        sendMessage(client, keyboard(code.unsignedIntValue, 1)); sendMessage(client, keyboard(code.unsignedIntValue, 2));
        if (paste) sendMessage(client, keyboard(0xe3, 2));
      }
    }
    [NSThread sleepForTimeInterval:0.15];
    puts("{\"ok\":true}");
    return 0;
  }
}
