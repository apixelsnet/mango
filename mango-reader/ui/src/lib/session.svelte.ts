// Who is using Mango: whether it asks for a password, and which account this browser is signed in to.

import { gate, get, message, send } from './api';

type Status = { login: boolean; setup: boolean; setupCode: boolean; signedIn: boolean; named: boolean; name: string | null; admin: boolean };

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
  signedIn: false,
  /** More than one account exists: signing in takes a name as well. */
  named: false,
  /** The account this browser is signed in to. */
  name: null as string | null,
  /** Whether that account may change what all accounts share: the accounts themselves, and the solver. */
  admin: false,
});

export async function loadSession() {
  try {
    Object.assign(session, await get<Status>('/auth'), { known: true, error: null });
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

export async function signOut() {
  await send('POST', '/auth/logout');
  location.reload();
}
