declare module "clipboard-event" {
  import { EventEmitter } from "node:events";

  export class ClipboardEventListener extends EventEmitter {
    constructor();
    startListening(): void;
    stopListening(): void;
  }

  const clipboardListener: ClipboardEventListener;
  export default clipboardListener;
}
