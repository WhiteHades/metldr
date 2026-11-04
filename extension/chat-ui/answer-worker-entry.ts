import { GuestSession, initSync } from "../../crates/metldr-answer-codec/pkg/answer_codec.js";
import installWorker from "./answer-worker.js";

installWorker((initialization, module) => {
  initSync({ module });
  return GuestSession.new(initialization);
});
