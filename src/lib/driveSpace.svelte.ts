/**
 * A counter the drive list watches: bump it when something that changes a drive's free space
 * has finished (a copy, move, delete, or emptying the Recycle Bin), so the sidebar looks again
 * without waiting for the window to regain focus.
 */
export const driveSpace = $state({ stamp: 0 });

export function driveSpaceChanged(): void {
  driveSpace.stamp++;
}
