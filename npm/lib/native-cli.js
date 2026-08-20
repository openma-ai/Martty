/** Owner has no painter/client tree; the native binary owns ACP directly. */
export function runsNativeOwner(argv) {
  return argv[0] === 'owner'
}
