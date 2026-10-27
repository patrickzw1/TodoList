// Synthetic STDIO protocol regression. This is not an actual AI-client UI test.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const root = await mkdtemp(join(tmpdir(), "todolist-mcp-v2-"));
const executable = join(root, "todolist-mcp.exe");
await copyFile(process.env.TODOLIST_MCP_EXECUTABLE ?? join(process.cwd(), "target/debug/todolist-mcp.exe"), executable);
const databasePath = join(root, "synthetic.sqlite");
const database = new DatabaseSync(databasePath);
database.exec("CREATE TABLE workspace_snapshot(id INTEGER PRIMARY KEY, version INTEGER NOT NULL, payload_json TEXT NOT NULL, updated_at INTEGER NOT NULL)");
const task = { id: "t", projectId: "p", title: "协议 验收任务", description: "synthetic body ".repeat(1000), status: "todo", priority: "medium", dueLabel: "未安排", dueDate: "9999-12-31", tags: [], source: "手动创建", archived: false, pinned: false, version: 1, subtasks: [], acceptanceCriteria: [], attachments: [], images: [], dependencies: [], activity: [] };
database.prepare("INSERT INTO workspace_snapshot VALUES(1,1,?,0)").run(JSON.stringify({ version: 1, projects: [{ id: "p", name: "合成协议", color: "#123456" }], tasks: [task] })); database.close();
const clients = [];
function start(actor, protocolVersion) {
  const child = spawn(executable, ["--client", actor], { cwd: root, env: { ...process.env, TODOLIST_DB_PATH: databasePath }, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  const exited = once(child, "exit"); let id = 0, buffer = "", stderr = ""; const pending = new Map();
  child.stderr.on("data", (s) => { stderr += s; });
  child.stdout.setEncoding("utf8"); child.stdout.on("data", (chunk) => { buffer += chunk; const lines = buffer.split(/\r?\n/); buffer = lines.pop(); for (const line of lines) { if (!line.trim()) continue; const result = JSON.parse(line); pending.get(result.id)?.(result); } });
  function request(method, params) { const requestId = ++id; return new Promise((resolve, reject) => { const timer = setTimeout(() => { pending.delete(requestId); reject(new Error(`${actor} ${method} timeout ${stderr.slice(0,200)}`)); }, 30_000); pending.set(requestId, (result) => { pending.delete(requestId); clearTimeout(timer); result.error ? reject(new Error(JSON.stringify(result.error))) : resolve(result.result); }); child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: requestId, method, params }) + "\n"); }); }
  const client = { actor, protocolVersion, request, child, exited, call: (name, args) => request("tools/call", { name, arguments: args }), async close() { child.stdin.end(); const timer = setTimeout(() => child.kill(), 3000); await exited; clearTimeout(timer); } };
  clients.push(client); return client;
}
function decoded(result) { assert.ok(!result.isError, result.content?.[0]?.text); return JSON.parse(result.content[0].text); }
const evidence = { scope: "raw isolated sidecar protocol; not actual-client rendering acceptance", clients: [], formats: {}, bytes: {} };
try {
  for (const [actor, protocol] of [["codex", "2025-06-18"], ["claude_code", "2025-06-18"], ["deepseek_harness", "2026-07-28"]]) {
    const client = start(actor, protocol); const init = await client.request("initialize", { protocolVersion: protocol, capabilities: {}, clientInfo: { name: "misleading_actor", version: "fixture" } });
    client.child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");
    const { tools } = await client.request("tools/list", {}); assert.equal(tools.length, 17); assert.ok(!tools.some((t) => /delete_task|delete_project/.test(t.name))); evidence.clients.push({ actor, protocol: init.protocolVersion, toolCount: tools.length, imageContent: false }); evidence.bytes.toolDefinitions = Buffer.byteLength(JSON.stringify(tools));
  }
  const c = clients[0];
  const baseline = decoded(await c.call("workspace_info", {})); assert.equal(baseline.channel, "development"); assert.equal(baseline.mcpVersion, "0.2.22"); assert.equal(baseline.buildIdentity, "todolist/0.2.22/development"); assert.ok(baseline.capabilities.includes("id_only_changes"));
  const summary = decoded(await c.call("list_tasks", { filter: { query: "协议验收任务", fields: ["title"], match_mode: "exact" } })); assert.equal(summary.tasks[0].id, "t"); assert.ok(!("description" in summary.tasks[0]));
  const full = decoded(await c.call("get_task", { task_id: "t" })); evidence.bytes.compactSearch = Buffer.byteLength(JSON.stringify(summary)); evidence.bytes.fullTask = Buffer.byteLength(JSON.stringify(full));
  const fileIds = {};
  for (const extension of ["pdf", "docx", "pptx", "xlsx", "png", "txt"]) {
    const latest = decoded(await c.call("get_task", { task_id: "t", fields: ["version"] })).task;
    const tool = extension === "png" ? "add_task_image" : "add_task_attachment";
    const imported = decoded(await c.call(tool, { task_id: "t", expected_version: latest.version, source_path: join(process.cwd(), "crates/todolist-mcp/tests/fixtures", `sample.${extension}`), request_id: `fixture-${extension}` }));
    assert.equal(Object.keys(imported.task).length, 2); assert.equal(imported.file.localPathStatus, "available"); fileIds[extension] = imported.file.id;
    const result = await c.call("read_task_file", { task_id: "t", file_id: imported.file.id }); const body = decoded(result);
    evidence.formats[extension] = { status: body.status, units: body.units.length, hasImage: result.content.some((block) => block.type === "image") };
    if (extension !== "png") assert.ok(body.units.length); else assert.equal(result.content[1].type, "image");
  }
  const originalImage = await readFile(join(process.cwd(), "crates/todolist-mcp/tests/fixtures/sample.png"));
  for (let i=0;i<clients.length;i++) { const result = await clients[i].call("read_task_file", { task_id: "t", file_id: fileIds.png, original: true }); const image = result.content.find((block) => block.type === "image"); assert.ok(image); assert.equal(image.mimeType, "image/png"); assert.deepEqual(Buffer.from(image.data,"base64"), originalImage); assert.ok(!result.content[0].text.includes(image.data)); evidence.clients[i].imageContent = true; }
  const wrong = await c.call("read_task_file", { task_id: "other", file_id: fileIds.pdf }); assert.equal(wrong.isError, true);
  const latest = decoded(await c.call("get_task", { task_id:"t",fields:["version"] })).task;
  const edit = { task_id:"t", expected_version:latest.version, request_id:"append-once",description_edit:{action:"append",text:"\n合成尾部"}, subtask_edits:[{action:"add",item_id:"stable",title:"独立子项"}] };
  const changed = decoded(await clients[1].call("update_task", edit)); assert.equal(decoded(await clients[1].call("update_task", edit)).replayed, true); evidence.bytes.localWrite = Buffer.byteLength(JSON.stringify(changed)); assert.ok(!("description" in changed.task));
  const desc = decoded(await c.call("get_task", { task_id:"t",sections:["description"],description_start:full.task.description.length,description_limit:128 })); assert.equal(desc.task.description,"\n合成尾部");
  const delta = decoded(await c.call("workspace_info", { since:baseline.syncCursor,limit:1 })); assert.equal(delta.requiresFullSync,false); assert.equal(delta.changes.length,1); assert.ok(delta.nextCursor); assert.ok(!JSON.stringify(delta.changes).includes("合成尾部"));
  const batch = { request_id:"archive",tasks:[{task_id:"t",expected_version:changed.task.version,archived:true}] }; const archived = decoded(await clients[2].call("batch_update",batch)); assert.equal(archived.atomic,true); assert.equal(decoded(await clients[2].call("batch_update",batch)).replayed,true);
  assert.equal(decoded(await c.call("list_tasks",{filter:{archived:"archived"}})).totalCount,1);
  decoded(await c.call("batch_update",{request_id:"restore",tasks:[{task_id:"t",expected_version:archived.tasks[0].version,archived:false}]}));
  let searchArgs = { task_ids:["t"], query:"page",file_name:"sample" }; let found=0, pages=0, final;
  do { final=decoded(await c.call("search_file_content",searchArgs)); found+=final.hits.length; pages++; searchArgs={...searchArgs,cursor:final.nextCursor}; assert.ok(pages<20); } while(final.nextCursor);
  assert.ok(found); assert.equal(final.fullySearched,false); assert.equal(final.partial,true); evidence.contentSearch={pages,found,fullySearched:final.fullySearched,reason:"image requires OCR"};
  const fullRequests=[];
  for(const local of [false,true]) {
    const current=decoded(await c.call("get_task",{task_id:"t",fields:["version"]})).task;
    const args={task_id:"t",expected_version:current.version,request_id:local?"review-local-full":"review-whole-full",response:"full",...(local?{description_edit:{action:"append",text:"\nreview-full-once"}}:{title:"review full metadata"})};
    const result=decoded(await c.call("update_task",args)); assert.equal(result.task.attachments.find(f=>f.id===fileIds.txt).localPathStatus,"available"); assert.ok(result.appliedTask); fullRequests.push({args,result});
  }
  const audit=new DatabaseSync(databasePath);
  const rows=audit.prepare("SELECT entity_id FROM mcp_idempotency WHERE request_id IN ('review-local-full','review-whole-full')").all(); audit.close(); assert.equal(rows.length,2);
  for(const row of rows) {
    assert.ok(!row.entity_id.includes("localPath")); assert.ok(!row.entity_id.includes(root)); assert.ok(!row.entity_id.includes(root.replaceAll("\\","\\\\")));
    const receipt=JSON.parse(row.entity_id); assert.deepEqual(Object.keys(receipt.task).sort(),["id","version"]); assert.ok(!("description" in receipt.task));
  }
  const textFile=fullRequests[1].result.task.attachments.find(f=>f.id===fileIds.txt); await rm(textFile.localPath);
  const missing=decoded(await c.call("update_task",fullRequests[0].args)); assert.equal(missing.replayed,true); assert.deepEqual(missing.appliedTask,fullRequests[0].result.appliedTask); assert.equal(missing.task.attachments.find(f=>f.id===fileIds.txt).localPathStatus,"missing");
  const beforeRemoval=decoded(await c.call("get_task",{task_id:"t",fields:["version"]})).task;
  decoded(await c.call("remove_task_files",{task_id:"t",expected_version:beforeRemoval.version,file_ids:[fileIds.png],request_id:"review-unlink-image"}));
  const unlinked=decoded(await c.call("update_task",fullRequests[1].args)); assert.equal(unlinked.replayed,true); assert.equal(unlinked.task.images.length,0); assert.equal(unlinked.task.description.split("review-full-once").length-1,1); assert.deepEqual(unlinked.appliedTask,fullRequests[1].result.appliedTask);
  evidence.reviewFixes={fullUpdateAndLocalPatch:true,actualReceiptRowsChecked:rows.length,storedFullTasks:false,storedRuntimePaths:false,missingPathRefreshed:true,removedAssociationNotReplayed:true};
  evidence.pdfReview=[];
  for (const name of ["form.pdf","mixed-image.pdf"]) {
    const current=decoded(await c.call("get_task",{task_id:"t",fields:["version"]})).task;
    const imported=decoded(await c.call("add_task_attachment",{task_id:"t",expected_version:current.version,source_path:join(process.cwd(),"crates/todolist-mcp/tests/fixtures",name),request_id:`review-${name}`}));
    const read=decoded(await c.call("read_task_file",{task_id:"t",file_id:imported.file.id})); const text=read.units.map(u=>u.text).join("\n"); assert.ok(text.includes("Visible heading"));
    const search=decoded(await c.call("search_file_content",{task_ids:["t"],file_name:name,query:name==="form.pdf"?"FORM_NEEDLE":"UNKNOWN_VISUAL_NEEDLE"}));
    if(name==="form.pdf") { assert.equal(read.status,"ok"); assert.ok(text.includes("FORM_NEEDLE")); assert.equal(search.hits.length,1); assert.equal(search.fullySearched,true); }
    else { assert.equal(read.status,"partial"); assert.equal(search.hits.length,0); assert.equal(search.fullySearched,false); assert.equal(search.partial,true); assert.ok(search.issues.length); }
    evidence.pdfReview.push({name,status:read.status,textPreserved:true,searchHits:search.hits.length,fullySearched:search.fullySearched,partial:search.partial});
  }
  await writeFile(join(process.cwd(),"target/MCP_PROTOCOL_EVIDENCE_2026-10-28.json"), JSON.stringify(evidence,null,2)+"\n"); console.log(JSON.stringify(evidence,null,2));
} finally { await Promise.allSettled(clients.filter((c)=>c.child.exitCode===null).map((c)=>c.close())); await rm(root,{recursive:true,force:true}); }
