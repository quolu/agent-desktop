import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { execute } from "./desktop.mjs";

const directory = mkdtempSync(join(tmpdir(), "jev-loop-test-"));
const entry = join(dirname(fileURLToPath(import.meta.url)), "run.mjs");
const log = join(directory, "commands.jsonl");
const preload = join(directory, "fetch.mjs");

/** Replays the CLI's observations and delivery results without a real application or model API. */
const fake = `
const { appendFileSync } = require('node:fs');
const { basename } = require('node:path');
const command = basename(process.argv[1]);
const args = process.argv.slice(2);
const mode = process.env.JEV_TEST_MODE;
appendFileSync(process.env.JEV_TEST_LOG, JSON.stringify({command,args})+'\\n');
const error = (code, retry='unsafe', delivery='delivery_uncertain') =>
  ({ok:false,error:{code,message:'試験の失敗',disposition:{retry,delivery}}});
let result = {ok:true,data:{disposition:{delivery:'delivered_verified',retry:'unsafe'}}};
const clicked = () => require('node:fs').readFileSync(process.env.JEV_TEST_LOG,'utf8').includes('"command":"click"');
const reads = () => require('node:fs').readFileSync(process.env.JEV_TEST_LOG,'utf8').split('"command":"snapshot"').length - 1;
const cut = (children) => ({ok:true,data:{window:{title:'試験'},complete:false,truncated:true,nodes_observed:3,tree:{role:'window',children}}});
if (command === 'snapshot' && mode === 'window_closed' && clicked()) result = error('WINDOW_NOT_FOUND','safe','not_delivered');
else if (command === 'snapshot' && mode === 'unreadable_after' && clicked()) result = error('TIMEOUT','unknown','unknown');
else if (command === 'snapshot' && (mode === 'never_whole' || (mode === 'cold_cut' && reads() === 1))) result = cut([]);
else if (command === 'snapshot' && mode === 'partly_read') result = cut([{ref_id:'@s1:e1',role:'button',name:'試験ボタン',available_actions:['Click']}]);
else if (command === 'snapshot' && mode === 'cold_timeout' && reads() <= 2) result = error('TIMEOUT','unknown','unknown');
else if (command === 'snapshot' && mode === 'sheet_cut' && args.includes('--surface')) result = error('TIMEOUT','unknown','unknown');
else if (command === 'snapshot' && mode === 'sheet_cut') result = {ok:true,data:{window:{title:'試験'},complete:true,tree:{
  role:'window',children:[
    {ref_id:'@s1:e1',role:'button',name:'試験ボタン',available_actions:['Click']},
    {role:'sheet',children:[{ref_id:'@s1:e2',role:'button',name:'保存',available_actions:['Click']}]}
  ]}}};
else if (command === 'snapshot') result = {ok:true,data:{window:{title:'試験'},tree:{
  role:'window',children:[
    {ref_id:'@s1:e1',role:'button',name:'試験ボタン',available_actions:['Click']},
    {ref_id:'@s1:e2',role:'textfield',name:'試験欄',value:'',available_actions:['SetValue']}
  ]}}};
if (command === 'click' && mode === 'click_error') result=error('ACTION_FAILED');
if (command === 'set-value') result= mode==='unsafe_type'
  ? error('TIMEOUT') : error('ACTION_NOT_SUPPORTED','safe','not_delivered');
if (command === 'clipboard-get') result={ok:true,data:{text:'元の文字'}};
if (command === 'clipboard-set' && mode==='clipboard_error' && args[0]==='試験')
  result=error('COPY_FAILED','safe','not_delivered');
if (command === 'get') result={ok:true,data:{value:mode==='paste_wrong'?'違う文字':'試験'}};
console.log(JSON.stringify(result));
`;
for (const command of ["snapshot", "click", "set-value", "focus", "clipboard-get", "clipboard-set", "press", "get"]) {
  writeFileSync(join(directory, command), fake);
}
writeFileSync(preload, `
let calls=0;
globalThis.fetch=async (_url,options)=>{
  calls++;
  const body=JSON.parse(options.body), mode=process.env.JEV_TEST_MODE;
  const operation=mode==='wait'?(calls<=3?'WAIT':'DONE'):mode==='done'||mode==='partly_read'||mode==='sheet_cut'||mode.startsWith('cold_')?'DONE':
    mode==='blocked'?'BLOCKED':mode==='click_error'||mode==='window_closed'||mode==='unreadable_after'?'CLICK':mode==='paste_correct'&&calls>1?'DONE':'TYPE_TEXT';
  const answers={};
  for (const [id,question] of Object.entries(body.questions)) {
    const ids=Object.keys(question.criteria);
    const selected=id==='operation'?operation:ids[0];
    answers[id]={choice:selected,confidence:1,probabilities:Object.fromEntries(ids.map(x=>[x,Number(x===selected)]))};
  }
  return {ok:true,status:200,json:async()=>({answers})};
};
`);

const run = (mode, extra = []) => {
  writeFileSync(log, "");
  const result = spawnSync(process.execPath, ["--import", pathToFileURL(preload).href, entry,
    "--app", "試験アプリ", ...extra, "--text", "試験", "試験を完了する"], {
    cwd: directory,
    encoding: "utf8",
    timeout: 10000,
    env: { ...process.env, TYPESAFE_API_KEY: "fixture", AGENT_DESKTOP_BIN: process.execPath,
      JEV_TEST_MODE: mode, JEV_TEST_LOG: log },
  });
  assert.ifError(result.error);
  assert.equal(result.stderr, "");
  const events = result.stdout.trim().split("\n").map(JSON.parse);
  const commands = readFileSync(log, "utf8").trim().split("\n").filter(Boolean).map(JSON.parse);
  return { ...result, events, commands, stop: events.at(-1) };
};

try {
  const started = performance.now();
  await execute("試験アプリ", "WAIT", null, null, null);
  assert.ok(performance.now() - started >= 230, "WAIT actually waits");

  const waited = run("wait");
  assert.equal(waited.status, 0);
  assert.equal(waited.stop.stop, "done");
  assert.equal(waited.events.filter(event => event.turn).length, 3);

  const failed = run("click_error");
  assert.equal(failed.status, 1);
  assert.equal(failed.stop.stop, "action_failed");
  assert.equal(failed.stop.error.code, "ACTION_FAILED");
  assert.equal(failed.stop.error.disposition.retry, "unsafe");
  assert.equal(failed.commands.filter(command => command.command === "click").length, 1);

  const unsafe = run("unsafe_type");
  assert.equal(unsafe.status, 1);
  assert.equal(unsafe.stop.error.code, "TIMEOUT");
  assert.equal(unsafe.commands.some(command => command.command === "focus" || command.command === "press"), false);

  const copyFailed = run("clipboard_error");
  assert.equal(copyFailed.status, 1);
  assert.equal(copyFailed.stop.error.code, "COPY_FAILED");
  assert.equal(copyFailed.commands.some(command => command.command === "press"), false);

  const wrongText = run("paste_wrong");
  assert.equal(wrongText.status, 1);
  assert.equal(wrongText.stop.error.code, "TEXT_VERIFICATION_FAILED");
  assert.equal(wrongText.commands.filter(command => command.command === "press").length, 1);

  const windowed = run("done", ["--window-id", "w-42"]);
  assert.equal(windowed.status, 0);
  const snapshots = windowed.commands.filter(command => command.command === "snapshot");
  assert.ok(snapshots.length > 0);
  for (const { args } of snapshots) assert.deepEqual(args.slice(args.indexOf("--window-id"), args.indexOf("--window-id") + 2), ["--window-id", "w-42"]);
  assert.equal(windowed.stop.screen.window_id, "w-42");
  assert.equal(run("done").commands.some(command => command.args.includes("--window-id")), false);

  const windowPaste = run("paste_correct", ["--window-id", "w-42"]);
  assert.equal(windowPaste.status, 0);
  const press = windowPaste.commands.find(command => command.command === "press");
  assert.deepEqual(press.args.slice(press.args.indexOf("--window-id"), press.args.indexOf("--window-id") + 2), ["--window-id", "w-42"]);

  const closed = run("window_closed", ["--window-id", "w-42"]);
  assert.equal(closed.status, 1);
  assert.equal(closed.stop.stop, "window_closed");
  assert.equal(closed.events.filter(event => event.turn).length, 1);
  assert.equal(closed.events.find(event => event.turn).turn.ok, true);
  const afterClick = closed.commands.slice(closed.commands.findIndex(command => command.command === "click") + 1);
  assert.equal(afterClick.filter(command => command.command === "snapshot").length, 1);

  const depth = (command) => command.args.includes("--max-depth");
  const coldCut = run("cold_cut");
  assert.equal(coldCut.status, 0, "a read cut short is taken again, not reported as an empty screen");
  assert.equal(coldCut.stop.stop, "done");
  assert.equal(coldCut.commands.filter(command => command.command === "snapshot").length, 2);
  assert.equal(coldCut.commands.some(depth), false);

  const coldTimeout = run("cold_timeout");
  assert.equal(coldTimeout.status, 0);
  assert.equal(coldTimeout.stop.stop, "done");
  assert.equal(coldTimeout.commands.filter(command => command.command === "snapshot").length, 3);
  assert.equal(coldTimeout.commands.some(depth), false);

  const neverWhole = run("never_whole");
  assert.equal(neverWhole.status, 1);
  assert.equal(neverWhole.stop.stop, "the screen was only partly read, and nothing in that part can be acted on");
  assert.equal(neverWhole.stop.screen.incomplete, true);
  assert.deepEqual(neverWhole.commands.map(command => command.command), ["snapshot", "snapshot", "snapshot"]);
  assert.equal(neverWhole.commands.some(depth), false);

  const partlyRead = run("partly_read");
  assert.equal(partlyRead.status, 0, "a tree that stays incomplete is still read, as it was before");
  assert.equal(partlyRead.stop.screen.incomplete, true, "and the stop says the screen was not whole");
  assert.equal(partlyRead.commands.filter(command => command.command === "snapshot").length, 3);
  assert.equal("incomplete" in run("done").stop.screen, false);

  const sheetCut = run("sheet_cut");
  assert.equal(sheetCut.stop.screen.surface, "sheet");
  assert.equal(sheetCut.stop.screen.incomplete, true, "a sheet that could not be read in time is not passed off as read");
  assert.equal(sheetCut.commands.filter(command => command.args.includes("--surface")).length, 3);

  const lost = run("unreadable_after");
  assert.equal(lost.status, 1);
  assert.equal(lost.stop.stop, "unreadable_after_action");
  assert.match(lost.stop.error, /TIMEOUT/);
  assert.equal(lost.stop.history.length, 1, "an action that ran is reported even when the screen cannot be read after it");
  const delivered = lost.events.find(event => event.turn).turn;
  assert.equal(delivered.ok, true);
  assert.equal(delivered.changed, null);
  assert.equal(lost.commands.filter(command => command.command === "click").length, 1);

  assert.equal(run("blocked").status, 1);
  assert.equal(run("done").status, 0);
  const pasted = run("paste_correct");
  assert.equal(pasted.status, 0);
  assert.equal(pasted.commands.filter(command => command.command === "press").length, 1);
  assert.equal(pasted.events[0].turn.delivery, "delivered_verified");
  console.log("ok");
} finally {
  rmSync(directory, { recursive: true, force: true });
}
