import { useSyncExternalStore } from "react";
import { superDriveApi } from "./superDriveApi";
import { createUploadQueue, type UploadQueue, type UploadQueueState } from "./queue/uploadQueue";

let sharedQueue: UploadQueue | null = null;

/** One upload queue per window, like the store. */
export function getUploadQueue(): UploadQueue {
  sharedQueue ??= createUploadQueue({ api: superDriveApi });
  return sharedQueue;
}

export function useUploadQueueState(): UploadQueueState {
  const queue = getUploadQueue();
  return useSyncExternalStore(queue.subscribe, queue.getState, queue.getState);
}
