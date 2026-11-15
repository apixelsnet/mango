// "Pick by hand" on the screen that moves many titles at once leads through search to the
// screen that moves one. These two keys (see keep.ts) bring the reader back once it moved.

/** `{ from, back }`: the title being picked by hand, and the address to return to after it moved. */
export const BY_HAND = 'migrate-many-by-hand';
/** The titles moved by hand since, for that screen to mark. */
export const MOVED = 'migrate-many-moved';
