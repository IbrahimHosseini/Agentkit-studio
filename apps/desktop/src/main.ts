import { mount, tick } from 'svelte';
import App from './App.svelte';
import './styles.css';
import { invoke, isTauri } from '@tauri-apps/api/core';
mount(App, { target: document.getElementById('app')! });

// Svelte mount schedules its initial effects. Keep the fallback until the DOM
// exists, then let native code reveal the window. Do not wait for an animation
// frame here: WebKit can throttle frames while the window is hidden.
void tick().then(async () => {
    if (!document.querySelector('#app .app-shell'))
        throw new Error('The interface did not mount.');
    document.getElementById('startup-status')?.remove();
    if (isTauri()) await invoke('frontend_ready');
});
