use std::{env, path::PathBuf, process::Command};

fn check(workload: &str, expected: &str) {
    let source = env::var_os("METLDR_CONCURRENCY_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("src/services/ConcurrencyManager.ts"));
    let script = format!(
        r#"
import {{ pathToFileURL }} from 'node:url';
const {{ concurrencyManager: manager }} = await import(pathToFileURL(process.argv[1]));
const tick = () => new Promise(resolve => setImmediate(resolve));
{workload}
"#
    );
    let output = Command::new("timeout")
        .args(["--signal=KILL", "5s", "node", "--input-type=module", "-e"])
        .arg(script)
        .arg(source.canonicalize().expect("actual service source"))
        .output()
        .expect("Node.js and timeout must be installed");
    assert!(
        output.status.success(),
        "workload failed or hung: {:?}\n{}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().last(), Some(expected), "{stdout}");
}

#[test]
fn duplicate_urls_yield_to_eligible_work_and_eventually_complete() {
    check(
        r#"
const gates = new Map();
const started = [];
const launch = (url, id) => {
  const gate = Promise.withResolvers();
  gates.set(id, gate);
  return manager.execute('indexing', url, async () => {
    started.push(id);
    await gate.promise;
    return id;
  });
};
const jobs = [launch('a','a'), launch('b','b'), launch('c','c'),
              launch('d','d1'), launch('d','d2')];
await tick();
gates.get('a').resolve(); await jobs[0]; await tick();
gates.get('b').resolve(); await jobs[1]; await tick();
jobs.push(launch('e','e')); await tick();
const progress = started.join(',');
gates.get('d1').resolve(); await jobs[3]; await tick();
for (const id of ['c','d2','e']) gates.get(id).resolve();
const results = await Promise.all(jobs);
console.log(JSON.stringify({progress, results, active:manager.getActiveCount('indexing'),
  queued:manager.getQueueLength('indexing')}));
"#,
        r#"{"progress":"a,b,c,d1,e","results":["a","b","c","d1","d2","e"],"active":0,"queued":0}"#,
    );
}

#[test]
fn abort_keeps_physical_ownership_and_cancels_all_queued_duplicates() {
    check(
        r#"
const old = Promise.withResolvers();
const replacement = Promise.withResolvers();
let signal;
let started = 0;
const first = manager.execute('indexing','same', async value => {
  signal = value; started++; await old.promise;
});
await tick();
manager.abort('indexing','same');
const second = manager.execute('indexing','same',async () => {
  started++; await replacement.promise;
});
await tick();
const before = [started,manager.getActiveCount('indexing'),manager.getQueueLength('indexing'),signal.aborted];
old.resolve(); await first; await tick();
const during = [started,manager.isRunning('indexing','same')];
const queued = [0,1].map(() => manager.execute('indexing','same',async () => { started++; }));
const cancelled = Promise.allSettled(queued);
manager.abort('indexing','same');
const cancellations = (await cancelled).map(result => result.status);
replacement.resolve(); await second;
const allGate = Promise.withResolvers();
const active = manager.execute('summary','all',async () => allGate.promise);
await tick();
const waiting = manager.execute('summary','all',async () => { started++; });
const waitingResult = Promise.allSettled([waiting]);
manager.abortAll('summary');
const retained = manager.getActiveCount('summary');
const allCancelled = (await waitingResult)[0].status;
allGate.resolve(); await active;
console.log(JSON.stringify({before,during,cancellations,retained,allCancelled,
  active:manager.getActiveCount('indexing') + manager.getActiveCount('summary'), started}));
"#,
        r#"{"before":[1,1,1,true],"during":[2,true],"cancellations":["rejected","rejected"],"retained":1,"allCancelled":"rejected","active":0,"started":2}"#,
    );
}

#[test]
fn synchronous_failure_and_reentry_preserve_url_and_capacity() {
    check(
        r#"
const gates = [Promise.withResolvers(),Promise.withResolvers()];
const order = [];
const active = gates.map((gate,index) => manager.execute('summary',String(index),async () => gate.promise));
await tick();
let sibling;
const failed = manager.execute('summary','reenter',() => {
  order.push('first');
  sibling = manager.execute('summary','reenter',async () => { order.push('second'); return 7; });
  throw new Error('expected callback failure');
});
const failure = failed.catch(error => error.message);
gates[0].resolve(); await active[0];
const message = await failure;
const value = await sibling;
gates[1].resolve(); await active[1];
console.log(JSON.stringify({order,message,value,status:manager.getStatus()}));
"#,
        r#"{"order":["first","second"],"message":"expected callback failure","value":7,"status":{"chat":{"active":0,"queued":0},"indexing":{"active":0,"queued":0},"summary":{"active":0,"queued":0}}}"#,
    );
}
