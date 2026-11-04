import type { GuestSession } from "../../crates/metldr-answer-codec/pkg/answer_codec.js";

export default function installWorker(createSession: (initialization: string, module: WebAssembly.Module) => GuestSession): void;
