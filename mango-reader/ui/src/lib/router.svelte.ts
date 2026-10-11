// Routes live after the # in the address, so the server only ever serves one page
// and Mango works under any sub-path.

export const route = $state({ path: '/', query: new URLSearchParams() });

function read() {
  const raw = location.hash.replace(/^#/, '') || '/';
  const cut = raw.indexOf('?');
  route.path = cut < 0 ? raw : raw.slice(0, cut);
  route.query = new URLSearchParams(cut < 0 ? '' : raw.slice(cut + 1));
}

addEventListener('hashchange', read);
read();

/** Opens a route. With `replace` the current one is not kept in the history. */
export function go(to: string, replace = false) {
  if (replace) location.replace('#' + to);
  else location.hash = to;
}
