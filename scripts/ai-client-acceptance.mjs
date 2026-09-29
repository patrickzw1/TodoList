// Optional real-client acceptance. All homes, configs, credentials and DBs are isolated.
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { createServer } from "node:http";

const root = await mkdtemp(join(tmpdir(), "todolist-real-clients-"));
const executable = join(root, "todolist-mcp.exe");
await copyFile(process.env.TODOLIST_MCP_EXECUTABLE ?? join(process.cwd(), "target/debug/todolist-mcp.exe"), executable);
const database = join(root, "fixture.sqlite");
const db = new DatabaseSync(database);
db.exec("CREATE TABLE workspace_snapshot (id INTEGER PRIMARY KEY CHECK (id=1), version INTEGER NOT NULL, payload_json TEXT NOT NULL, updated_at INTEGER NOT NULL)");
db.prepare("INSERT INTO workspace_snapshot VALUES(1,1,?,0)").run(JSON.stringify({ version: 1, projects: [], tasks: [] })); db.close();
const safeEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/KEY|TOKEN|SECRET|PASSWORD|ANTHROPIC|CLAUDE|CODEX|DSH/i.test(key)));
const env = { ...safeEnv, HOME: root, USERPROFILE: root, APPDATA: join(root, "Roaming"), LOCALAPPDATA: join(root, "Local"), CODEX_HOME: join(root,"codex"), CLAUDE_CONFIG_DIR: join(root,"claude"), DSH_HOME: join(root,"dsh"), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1", DISABLE_TELEMETRY: "1", TODOLIST_DB_PATH: database };
await Promise.all(["codex","claude","dsh","work","Local","Roaming"].map((dir) => mkdir(join(root,dir), {recursive:true})));
const evidence = { codex: { available: false }, claude_code: { available: false }, deepseek_harness: { available: null, discoveryProven: false, limitation: "This script does not check or exercise the DSH desktop/embedded runtime; raw sidecar negotiation is not a DSH client acceptance." } };
const owned = [];
let mockProvider;
function launch(command,args) {
  const child = spawn(command,args,{cwd:join(root,"work"),env,windowsHide:true,stdio:["pipe","pipe","pipe"]});
  const exit = once(child,"exit"); owned.push({ child, exit }); return { child, exit };
}
async function run(command,args,allowFailure=false) {
  const {child,exit}=launch(command,args); let stdout="",stderr="";
  child.stdout.on("data",(s)=>stdout+=s); child.stderr.on("data",(s)=>stderr+=s); child.stdin.end();
  const timeout=setTimeout(()=>child.kill(),30_000); const [code]=await exit; clearTimeout(timeout);
  if(code!==0 && !allowFailure) throw new Error(`Client command failed (${code}): ${stderr.slice(0,800)}`);
  return stdout;
}
try {
  const claude=process.env.TODOLIST_CLAUDE_EXECUTABLE;
  if(claude) {
    evidence.claude_code.available=true;
    evidence.claude_code.version=(await run(claude,["--version"])).trim();
    await run(claude,["mcp","add","--scope","user","--transport","stdio","todolist","--",executable,"--client","claude_code"]);
    const configPath=join(env.CLAUDE_CONFIG_DIR,".claude.json");
    const doc=JSON.parse(await readFile(configPath,"utf8"));
    if(doc.mcpServers?.todolist?.command!==executable) throw new Error("Claude's redirected user config path was not confirmed");
    evidence.claude_code.customConfigRelativePath="CLAUDE_CONFIG_DIR/.claude.json";
    const debug=join(root,"claude-mcp.log");
    const listing=await run(claude,["--debug-file",debug,"mcp","list"]);
    evidence.claude_code.connected=/todolist:.*Connected/i.test(listing);
    let log="";try {log=await readFile(debug,"utf8");} catch {}
    const toolCount=/Connected.*?(\d+) tools/i.exec(log) ?? /tools[:=]\s*(\d+)/i.exec(log);
    evidence.claude_code.toolCount=toolCount?Number(toolCount[1]):null;
    evidence.claude_code.discoveryProven=evidence.claude_code.connected && evidence.claude_code.toolCount===10;
    if(!evidence.claude_code.discoveryProven) {
      // A deterministic loopback provider permits client startup/tool discovery
      // without credentials, real inference, or any external provider request.
      let providerRequests=0;
      mockProvider=createServer((request,response)=>{
        providerRequests++; request.resume();
        if(!request.url.startsWith("/v1/messages")){response.writeHead(404);response.end("fixture");return;}
        response.writeHead(200,{"Content-Type":"text/event-stream","Connection":"close"});
        const events=[
          ["message_start",{type:"message_start",message:{id:"fixture-msg",type:"message",role:"assistant",model:"fixture",content:[],stop_reason:null,stop_sequence:null,usage:{input_tokens:1,output_tokens:0}}}],
          ["content_block_start",{type:"content_block_start",index:0,content_block:{type:"text",text:""}}],
          ["content_block_delta",{type:"content_block_delta",index:0,delta:{type:"text_delta",text:"Fixture complete."}}],
          ["content_block_stop",{type:"content_block_stop",index:0}],
          ["message_delta",{type:"message_delta",delta:{stop_reason:"end_turn",stop_sequence:null},usage:{output_tokens:1}}],
          ["message_stop",{type:"message_stop"}],
        ];
        for(const [event,data] of events) response.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
        response.end();
      });
      mockProvider.listen(0,"127.0.0.1");await once(mockProvider,"listening");
      env.ANTHROPIC_BASE_URL=`http://127.0.0.1:${mockProvider.address().port}`;
      env.ANTHROPIC_API_KEY="fixture-only-key";
      let output="";
      try { output=await run(claude,["--print","--output-format","stream-json","--verbose","--max-turns","1","--no-session-persistence","--strict-mcp-config","--mcp-config",JSON.stringify({mcpServers:{todolist:doc.mcpServers.todolist}}),"--","Reply with fixture complete; do not call tools."],true); }
      catch(error) { evidence.claude_code.mockStartupError=error.message.slice(0,500); }
      const init=output.split(/\r?\n/).filter(Boolean).map(line=>{try{return JSON.parse(line);}catch{return null;}}).find(message=>message?.type==="system"&&message.subtype==="init");
      const tools=init?.tools?.filter(name=>name.startsWith("mcp__todolist__"))??[];
      evidence.claude_code.toolCount=tools.length||evidence.claude_code.toolCount;
      evidence.claude_code.discoveryProven=tools.length===10;
      evidence.claude_code.provider="deterministic loopback fixture"; evidence.claude_code.providerRequests=providerRequests;
      mockProvider.closeAllConnections();await new Promise(resolve=>mockProvider.close(resolve));mockProvider=null;
      delete env.ANTHROPIC_API_KEY;delete env.ANTHROPIC_BASE_URL;
    }
    if(!evidence.claude_code.discoveryProven) evidence.claude_code.limitation="Health check passed; full tool discovery was not proven by this installed CLI.";
  }
  const codex=process.env.TODOLIST_CODEX_EXECUTABLE;
  if(codex) {
    evidence.codex.available=true; evidence.codex.version=(await run(codex,["--version"])).trim();
    await writeFile(join(env.CODEX_HOME,"config.toml"), `[mcp_servers.todolist]\ncommand = ${JSON.stringify(executable)}\nargs = ["--client", "codex"]\nstartup_timeout_sec = 30\n[mcp_servers.todolist.env]\nTODOLIST_DB_PATH = ${JSON.stringify(database)}\n`);
    const {child,exit}=launch(codex,["app-server"]);
    const pending=new Map();let next=0,buffer="",stderr="";
    child.stderr.on("data",s=>stderr+=s);
    child.stdout.setEncoding("utf8");child.stdout.on("data",s=>{
      buffer+=s;const lines=buffer.split(/\r?\n/);buffer=lines.pop();
      for(const line of lines) {if(!line.trim())continue;const message=JSON.parse(line);const callback=pending.get(message.id);if(callback){pending.delete(message.id);callback(message);}}
    });
    const request=(method,params)=>new Promise((resolve,reject)=>{
      const id=++next;const timeout=setTimeout(()=>{pending.delete(id);reject(new Error(`Codex ${method} timed out: ${stderr.slice(0,400)}`));},30_000);
      pending.set(id,message=>{clearTimeout(timeout);message.error?reject(new Error(JSON.stringify(message.error))):resolve(message.result);});
      child.stdin.write(JSON.stringify({id,method,params})+"\n");
    });
    await request("initialize",{clientInfo:{name:"todolist_fixture",version:"0.2.19"},capabilities:{experimentalApi:true}});
    child.stdin.write(JSON.stringify({method:"initialized"})+"\n");
    const result=await request("mcpServerStatus/list",{});
    const server=result.data?.find(s=>s.name==="todolist");
    evidence.codex.toolCount=server?Object.keys(server.tools??{}).length:0;
    evidence.codex.discoveryProven=evidence.codex.toolCount===10;
    evidence.codex.serverState=server?.status ?? server?.state ?? null;
    child.stdin.end();const timeout=setTimeout(()=>child.kill(),3000);await exit;clearTimeout(timeout);
    if(!evidence.codex.discoveryProven) evidence.codex.limitation="App-server status returned without the complete TodoList tool set.";
  }
  await writeFile(join(process.cwd(),"target/AI_CLIENT_ACCEPTANCE.json"),JSON.stringify(evidence,null,2)+"\n");
  console.log(JSON.stringify(evidence,null,2));
} finally {
  if(mockProvider){mockProvider.closeAllConnections();await new Promise(resolve=>mockProvider.close(resolve));}
  await Promise.allSettled(owned.filter(({child})=>child.exitCode===null).map(async({child,exit})=>{child.stdin.end();const timeout=setTimeout(()=>child.kill(),2000);await exit;clearTimeout(timeout);}));
  await rm(root,{recursive:true,force:true});
}
