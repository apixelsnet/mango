// Data saver: pages come smaller from Mango, as JPEGs no wider than a phone shows them.
// It is this device's say, not the account's: a phone on mobile data wants it, the same
// reader's computer at home does not. Kept in the browser's own storage.

const KEY = 'mango.dataSaver';

/** Whether pages are asked for smaller on this device. */
export function savingData(): boolean {
  try {
    return localStorage.getItem(KEY) === '1';
  } catch {
    return false;
  }
}

export function setSavingData(on: boolean) {
  try {
    if (on) localStorage.setItem(KEY, '1');
    else localStorage.removeItem(KEY);
  } catch {
    // Storage turned off: the setting lasts as long as the page.
  }
}

/** What a page's address carries when data is saved. */
export const lightParam = () => (savingData() ? '&light=1' : '');
