import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { loadPrefs } from './lib/prefs.svelte';
import { loadSession, session } from './lib/session.svelte';
import { adoptServerTheme, initTheme } from './lib/theme.svelte';

// A tracker that signs in through its own page sends the browser back here with its
// answer in the address. It is put aside for the Trackers settings, which finish the
// sign-in, and the address is tidied so a reload does not send it twice.
const answer = new URLSearchParams(location.search);
if (answer.has('code') && answer.has('state')) {
  sessionStorage.setItem('tracker.answer', JSON.stringify({ code: answer.get('code'), state: answer.get('state') }));
  history.replaceState(null, '', location.pathname + '#/settings/trackers');
  // The router read the address when it was loaded, and this change is a silent one.
  dispatchEvent(new HashChangeEvent('hashchange'));
}

// The theme is applied from this browser's copy first, so the first paint is right
// before the server has answered. The server's copy then takes over, once Mango
// has said this browser is signed in.
initTheme();
loadSession().then(() => {
  if (session.signedIn) loadPrefs().then(adoptServerTheme);
});

mount(App, { target: document.getElementById('app')! });
