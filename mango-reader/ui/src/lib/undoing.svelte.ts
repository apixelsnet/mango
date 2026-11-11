// A short word at the foot of the screen after something was done, for everyone and for a
// screen reader alike (lib/Undo.svelte, shown by App.svelte). An action that can be taken
// back is not sent at once: it shows as done, waits while the word is up, and is sent when
// the word goes, when another action comes, or when the screen is left. Undo puts it back
// as it was and nothing is sent.

import { message, pending } from './api';

type Held = {
  /** Puts the screen back as it was before the action. */
  undo: () => void;
  /**
   * Sends the action, and only that: reads made inside it would wait for themselves (api.ts).
   * A failure is told in the same place.
   */
  commit: () => Promise<unknown>;
  /** Once it is sent, such as reading the screen's list again. */
  then?: () => void;
  /** When sending failed: put the screen back as Mango has it. */
  failed?: () => void;
};

/** What the bar shows: the text, and whether Undo is offered with it. */
export const word = $state({ text: '', undoable: false, turn: 0 });

let held: Held | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;

function show(text: string, undoable: boolean, ms: number) {
  clearTimeout(timer);
  word.text = text;
  word.undoable = undoable;
  word.turn++;
  timer = setTimeout(settle, ms);
}

/** Says what happened, with nothing to take back. */
export function tell(text: string) {
  settle();
  show(text, false, 4000);
}

/** Shows `text` with Undo. `commit` is sent a few seconds later unless Undo is pressed. */
export function offer(text: string, action: Held) {
  settle();
  held = action;
  show(text, true, 7000);
}

/** Sends what is held back, now. Called when the word goes and when the screen changes. */
export function settle() {
  clearTimeout(timer);
  const action = held;
  held = null;
  word.text = '';
  word.undoable = false;
  if (!action) return;
  const work = action.commit();
  pending.change = work.catch(() => {});
  work.then(
    () => action.then?.(),
    (e) => {
      action.failed?.();
      show(`That did not work: ${message(e)}`, false, 6000);
    },
  );
}

/** Takes the held action back. */
export function undo() {
  const action = held;
  held = null;
  clearTimeout(timer);
  if (!action) return;
  action.undo();
  show('Undone.', false, 2500);
}

// Leaving the page sends what is held: api.ts marks a call made then to outlive the page.
addEventListener('pagehide', settle);
// So does going to another screen, before that screen asks for anything: its reads then
// wait for the change (api.ts), so they show it.
addEventListener('hashchange', () => {
  if (held) settle();
});
