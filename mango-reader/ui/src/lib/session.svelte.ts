// Who is using Mango: whether it asks for a password, and which account this browser is signed in to.

import { gate, get, message, send } from './api';
import { forgetAll, noteWho } from './offline';
import { dropAll } from './unsent';
import { turnOff } from './push';

type Status = { login: boolean; setup: boolean; setupCode: boolean; signup: boolean; signedIn: boolean; named: boolean; name: string | null; id?: number; admin: boolean; account?: string };

export const session = $state({
  /** False until Mango has said who we are. */
  known: false,
  /** Set when Mango could not be asked. */
  error: null as string | null,
  login: true,
  /** No password exists yet: the first thing to do is set one. */
  setup: false,
  /** Setting it takes the setup code too: this browser is not on Mango's own machine. */
  setupCode: false,
  /** Whoever holds an admin's invitation can make an account: the main page is shown, and offers it. */
  signup: false,
  signedIn: false,
  /** More than one account exists: signing in takes a name as well. */
  named: false,
  /** The account this browser is signed in to. */
  name: null as string | null,
  /** Its number. Each account numbers its chapters from 1: page addresses carry this, so the browser keeps them apart. */
  id: null as number | null,
  /** Whether that account may change what all accounts share: the accounts themselves, and the solver. */
  admin: false,
  /** Where this reader's account is kept, when whoever runs this Mango keeps one: their plan, billing and the like. */
  account: null as string | null,
});

export async function loadSession() {
  try {
    Object.assign(session, { id: null }, await get<Status>('/auth'), { known: true, error: null });
    // Signed in when no password is asked for, too: the owner's library is open to this browser.
    await noteWho(session.signedIn || !session.login).catch(() => {});
  } catch (e) {
    session.error = message(e);
  }
}

// Any call can find the session gone: it ran out, or the password changed elsewhere.
gate.lost = (code) => {
  session.signedIn = false;
  session.setup = code === 'setup_required';
};

/** Signs in, or with `first` sets the password. The page starts over once it worked. */
export async function signIn(password: string, first: boolean, name = '', code = '') {
  await send('POST', first ? '/auth/setup' : '/auth/login', { password, name, code });
  location.reload();
}

/** Makes an account with an invitation's code and signs in to it. The page starts over, as after signing in. */
export async function signUp(name: string, password: string, code: string) {
  await send('POST', '/auth/signup', { name, password, code });
  location.reload();
}

export async function signOut() {
  // Whoever signs in next on this device is not to hear of this account's chapters.
  await turnOff().catch(() => {});
  // Chapters kept on this device go too, before Mango is asked: whoever signs in next is not
  // to read them, even when the sign-out call itself fails for want of a connection.
  await forgetAll().catch(() => {});
  dropAll();
  await send('POST', '/auth/logout');
  location.reload();
}
