// Notifications on this device: word of new chapters through the browser's push service,
// shown by public/sw.js. Each device turns them on or off for itself; Mango keeps one
// subscription per device and sends to all of an account's devices.

import { get, send } from './api';

/** Why this device cannot have notifications, or null when it can. */
export function unable(): string | null {
  const apple = /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.userAgent.includes('Mac') && navigator.maxTouchPoints > 1);
  const installed = matchMedia('(display-mode: standalone)').matches || (navigator as { standalone?: boolean }).standalone === true;
  if (!isSecureContext) return 'Notifications need Mango to be opened over HTTPS.';
  if (apple && !installed) return 'On an iPhone or iPad, add Mango to the Home Screen first (Share, then Add to Home Screen), and turn this on there.';
  if (!('serviceWorker' in navigator) || !('PushManager' in window) || !('Notification' in window)) return 'This browser cannot show notifications from a site.';
  return null;
}

type Status = { key: string; devices: number };

const bytes = (text: string) => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));

function same(a: ArrayBuffer | null, b: Uint8Array) {
  if (!a || a.byteLength !== b.length) return false;
  const view = new Uint8Array(a);
  return view.every((value, i) => value === b[i]);
}

/** The worker's registration, waited for with `wait` (it never comes when the worker could not start), and this device's subscription. */
async function subscription(wait = false) {
  const registration = wait ? await navigator.serviceWorker.ready : await navigator.serviceWorker.getRegistration();
  return { registration, current: (await registration?.pushManager.getSubscription()) ?? null };
}

/** Whether this device hears of new chapters for the account signed in, and how many devices do. */
export async function state(): Promise<{ on: boolean; devices: number }> {
  const status = await get<Status>('/push');
  if (unable() || Notification.permission !== 'granted') return { on: false, devices: status.devices };
  const { current } = await subscription();
  // A subscription made for another account on this browser is not this one's.
  return { on: !!current && same(current.options.applicationServerKey, bytes(status.key)), devices: status.devices };
}

/** Asks the browser's leave, subscribes and tells Mango. Throws with a reason to show when it cannot. */
export async function turnOn() {
  const why = unable();
  if (why) throw new Error(why);
  if ((await Notification.requestPermission()) !== 'granted') throw new Error('Notifications are blocked for this site. Allow them in the browser’s settings, then try again.');
  const { key } = await get<Status>('/push');
  const { registration, current } = await subscription(true);
  if (!registration) throw new Error('This browser cannot show notifications from a site.');
  if (current && !same(current.options.applicationServerKey, bytes(key))) await current.unsubscribe();
  const made = (current && same(current.options.applicationServerKey, bytes(key)) ? current : null) ?? (await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: bytes(key) }));
  await send('PUT', '/push', made.toJSON());
}

/** Unsubscribes this device and tells Mango. */
export async function turnOff() {
  if (unable()) return;
  const { current } = await subscription();
  if (!current) return;
  await send('DELETE', '/push', { endpoint: current.endpoint }).catch(() => {});
  await current.unsubscribe();
}

/** Sends a note to every device of the account. Answers how many took it. */
export async function test(): Promise<number> {
  return (await send<{ sent: number }>('POST', '/push/test')).sent;
}
