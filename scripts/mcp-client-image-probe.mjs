// Real installed client protocol probes using only isolated homes and localhost models.
// No credentials, paid inference, production DB, installed configuration or UI claims.
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createServer } from "node:http";
import { DatabaseSync } from "node:sqlite";
import { createHash } from "node:crypto";

const workspace = process.cwd();
const root = await mkdtemp(join(tmpdir(), "todolist-client-images-"));
const schemaRoot = join(workspace, "target", "mcp-client-image-schema");
const reportPath = join(workspace, "target", "MCP_CLIENT_IMAGE_PROBE.json");
const safeEnv = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/KEY|TOKEN|SECRET|PASSWORD|ANTHROPIC|CLAUDE|CODEX|DSH|PROXY/i.test(key)));
const env = { ...safeEnv, HOME: root, USERPROFILE: root, APPDATA: join(root,"Roaming"), LOCALAPPDATA: join(root,"Local"), CODEX_HOME: join(root,"codex"), CLAUDE_CONFIG_DIR: join(root,"claude"), DSH_HOME: join(root,"dsh"), CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC:"1", DISABLE_TELEMETRY:"1", HTTP_PROXY:"http://127.0.0.1:1", HTTPS_PROXY:"http://127.0.0.1:1", ALL_PROXY:"http://127.0.0.1:1", NO_PROXY:"127.0.0.1,localhost" };
await Promise.all(["codex","claude","dsh","work","Local","Roaming"].map(name => mkdir(join(root,name),{recursive:true})));
const children = [];
const evidence = { scope:"Installed clients process MCP protocol image content; visual UI rendering is not asserted.", codex:{},claude_code:{},deepseek_harness:{verified:false,limitation:"No installed DSH CLI was discovered; a raw sidecar --client flag is not real-client acceptance."} };
let provider;

function launch(command,args,extraEnv={}) {
  const child = spawn(command,args,{cwd:join(root,"work"),env:{...env,...extraEnv},windowsHide:true,stdio:["pipe","pipe","pipe"]});
  const done = once(child,"exit"); children.push({child,done});
  return {child,done};
}
async function run(command,args,timeoutMs=30000) {
  const {child,done}=launch(command,args); let stdout="",stderr="";
  child.stdout.on("data",chunk=>{if(stdout.length<2_000_000)stdout+=chunk;});
  child.stderr.on("data",chunk=>{if(stderr.length<100_000)stderr+=chunk;}); child.stdin.end();
  const timer=setTimeout(()=>child.kill(),timeoutMs);const [code]=await done;clearTimeout(timer);
  if(code!==0){const error=new Error(`Isolated client exited ${code}: ${stderr.slice(0,600)}`);error.fixtureStdout=stdout;throw error;}
  return stdout;
}
function rpc(command,args,extraEnv={}) {
  const {child,done}=launch(command,args,extraEnv);const pending=new Map();let sequence=0,buffer="",stderr="";
  child.stderr.on("data",chunk=>{if(stderr.length<100_000)stderr+=chunk;});
  child.stdout.setEncoding("utf8");child.stdout.on("data",chunk=>{
    buffer+=chunk;if(buffer.length>2_000_000){child.kill();return;}
    const lines=buffer.split(/\r?\n/);buffer=lines.pop();
    for(const line of lines){if(!line.trim())continue;let message;try{message=JSON.parse(line);}catch{continue;}const callback=pending.get(message.id);if(callback){pending.delete(message.id);callback(message);}}
  });
  return {child,done,notify:(method,params)=>child.stdin.write(JSON.stringify({jsonrpc:"2.0",method,params})+"\n"),request:(method,params)=>new Promise((resolve,reject)=>{
    const id=++sequence;const timer=setTimeout(()=>{pending.delete(id);reject(new Error(`${method} timed out: ${stderr.slice(0,400)}`));},30000);
    pending.set(id,message=>{clearTimeout(timer);message.error?reject(new Error(JSON.stringify(message.error))):resolve(message.result);});child.stdin.write(JSON.stringify({jsonrpc:"2.0",id,method,params})+"\n");
  })};
}
async function stop(connection) {
  if(connection.child.exitCode!==null)return;
  connection.child.stdin.end();const timer=setTimeout(()=>connection.child.kill(),3000);await connection.done;clearTimeout(timer);
}
function parsedTool(result) {
  if(result.isError)throw new Error(`Fixture MCP setup failed: ${result.content?.find(block=>block.type==="text")?.text?.slice(0,500)}`);
  return JSON.parse(result.content.find(block=>block.type==="text").text);
}
function imageSummary(block) {
  const data=block.type==="image"?(block.data??block.source?.data):null;
  const mime=block.mimeType??block.source?.media_type;
  if(!data)return null;
  const bytes=Buffer.from(data,"base64");return {mimeType:mime,bytes:bytes.length,sha256:createHash("sha256").update(bytes).digest("hex"),pngHeader:bytes.subarray(0,8).equals(Buffer.from([137,80,78,71,13,10,26,10]))};
}
function nestedImages(value,result=[]) {
  if(Array.isArray(value))value.forEach(item=>nestedImages(item,result));
  else if(value&&typeof value==="object"){const summary=imageSummary(value);if(summary)result.push(summary);Object.values(value).forEach(item=>nestedImages(item,result));}
  return result;
}
async function files(directory) {
  const result=[];
  for(const entry of await readdir(directory,{withFileTypes:true})) {
    const path=join(directory,entry.name);if(entry.isDirectory())result.push(...await files(path));else result.push(path);
  }
  return result;
}
try {
  const codex = process.env.TODOLIST_CODEX_EXECUTABLE;
  if(codex) {
    evidence.codex.version=(await run(codex,["--version"])).trim();
    await mkdir(schemaRoot,{recursive:true});
    await run(codex,["app-server","generate-json-schema","--out",schemaRoot,"--experimental"]);
    const methods=new Set();
    const collect=value=>{if(Array.isArray(value))value.forEach(collect);else if(value&&typeof value==="object"){if(value.properties?.method?.const)methods.add(value.properties.method.const);if(value.properties?.method?.enum)value.properties.method.enum.forEach(method=>methods.add(method));Object.values(value).forEach(collect);}};
    for(const path of await files(schemaRoot))if(path.endsWith(".json"))collect(JSON.parse(await readFile(path,"utf8")));
    evidence.codex.mcpRpcMethods=[...methods].filter(method=>/mcp/i.test(method)).sort();
    evidence.codex.turnRpcMethods=[...methods].filter(method=>/^(thread\/start|turn\/start|thread\/resume)$/.test(method));
  }
  if(process.argv.includes("--schema-only")) {
    await writeFile(reportPath,JSON.stringify(evidence,null,2)+"\n");
    console.log(JSON.stringify(evidence,null,2));
  } else {
    const executable=join(root,"todolist-mcp.exe"),database=join(root,"fixture.sqlite");
    await copyFile(process.env.TODOLIST_MCP_EXECUTABLE??join(workspace,"target/debug/todolist-mcp.exe"),executable);
    env.TODOLIST_DB_PATH=database;
    const db=new DatabaseSync(database);db.exec("CREATE TABLE workspace_snapshot(id INTEGER PRIMARY KEY CHECK(id=1),version INTEGER NOT NULL,payload_json TEXT NOT NULL,updated_at INTEGER NOT NULL)");
    db.prepare("INSERT INTO workspace_snapshot VALUES(1,1,?,0)").run(JSON.stringify({version:1,projects:[],tasks:[]}));db.close();
    const fixture=rpc(executable,["--client","codex"]);
    await fixture.request("initialize",{protocolVersion:"2025-03-26",capabilities:{},clientInfo:{name:"isolated_image_fixture_setup",version:"1"}});fixture.notify("notifications/initialized",{});
    const call=(name,args)=>fixture.request("tools/call",{name,arguments:args}).then(parsedTool);
    const project=await call("create_project",{name:"Image protocol fixture",request_id:"image-probe-project"});
    const task=await call("create_task",{project_id:project.project.id,title:"Isolated image protocol",request_id:"image-probe-task"});
    const imagePath=join(root,"fixture.png");await copyFile(join(workspace,"crates/todolist-mcp/tests/fixtures/sample.png"),imagePath);
    await stop(fixture);
    // This probe accepts reads through real clients. Atomic file import has its
    // own tests; directly seed this synthetic association to avoid conflating it
    // with the sandbox's Windows tempfile-rename permission boundary.
    const fileId="fixture-image",storageKey=`images/${fileId}.png`;
    await mkdir(join(root,"managed-files","images"),{recursive:true});await copyFile(imagePath,join(root,"managed-files",storageKey));
    const fixtureDb=new DatabaseSync(database);const row=fixtureDb.prepare("SELECT payload_json FROM workspace_snapshot WHERE id=1").get();
    const snapshot=JSON.parse(row.payload_json),stored=snapshot.tasks.find(item=>item.id===task.task.id);
    stored.images=[{id:fileId,originalName:"fixture.png",mediaType:"image/png",size:(await readFile(imagePath)).length,storageKey,addedAt:new Date().toISOString()}];stored.version++;snapshot.version++;
    fixtureDb.prepare("UPDATE workspace_snapshot SET version=?,payload_json=?,updated_at=? WHERE id=1").run(snapshot.version,JSON.stringify(snapshot),Date.now());fixtureDb.close();
    const argumentsValue={task_id:task.task.id,file_id:fileId,original:true};
    const expectedBytes=await readFile(imagePath);const expectedHash=createHash("sha256").update(expectedBytes).digest("hex");
    evidence.fixture={imageBytes:expectedBytes.length,imageSha256:expectedHash,taskAndFileIdsEphemeral:true,seed:"Isolated MCP-created task plus synthetic managed image association/file; file import is tested separately"};
    if(codex) {
      await writeFile(join(env.CODEX_HOME,"config.toml"),`model = "fixture"\nmodel_provider = "fixture"\n[model_providers.fixture]\nname = "No inference fixture"\nbase_url = "http://127.0.0.1:1/v1"\nwire_api = "responses"\nrequires_openai_auth = false\n[analytics]\nenabled = false\n[mcp_servers.todolist]\ncommand = ${JSON.stringify(executable)}\nargs = ["--client","codex"]\nstartup_timeout_sec = 30\n[mcp_servers.todolist.env]\nTODOLIST_DB_PATH = ${JSON.stringify(database)}\n`);
      const connection=rpc(codex,["app-server"]);
      try {
        await connection.request("initialize",{clientInfo:{name:"todolist_image_probe",version:"1"},capabilities:{experimentalApi:true}});connection.notify("initialized",{});
        const started=await connection.request("thread/start",{cwd:join(root,"work"),ephemeral:true,approvalPolicy:"never",sandbox:"read-only",model:"fixture",modelProvider:"fixture"});
        const response=await connection.request("mcpServer/tool/call",{threadId:started.thread.id,server:"todolist",tool:"read_task_file",arguments:argumentsValue});
        const images=nestedImages(response.content);evidence.codex.imageBlocks=images;
        evidence.codex.verified=!response.isError&&images.some(image=>image.mimeType==="image/png"&&image.sha256===expectedHash);
        evidence.codex.transport="Installed app-server mcpServer/tool/call RPC; no model turn or provider request";
        const text=response.content.find(block=>block.type==="text")?.text;const metadata=text?JSON.parse(text):null;
        evidence.codex.metadataStatus=metadata?.status;
      }catch(error){evidence.codex.verified=false;evidence.codex.limitation=error.message.slice(0,800);}finally{await stop(connection);}
    }
    const claude=process.env.TODOLIST_CLAUDE_EXECUTABLE;
    if(claude) {
      evidence.claude_code.version=(await run(claude,["--version"])).trim();
      let messageRequests=0,allRequests=0,blockedProxyRequests=0,firstTools=[],receivedImages=[],toolResultObserved=false;
      provider=createServer(async(request,response)=>{
        allRequests++;let body="";for await(const chunk of request){body+=chunk;if(body.length>4_000_000){response.writeHead(413);response.end();return;}}
        const url=new URL(request.url,`http://127.0.0.1:${provider.address().port}`);
        if(!["127.0.0.1","localhost"].includes(url.hostname)){blockedProxyRequests++;response.writeHead(403);response.end("External network disabled in fixture");return;}
        if(url.pathname.startsWith("/v1/messages/count_tokens")){response.writeHead(200,{"Content-Type":"application/json"});response.end(JSON.stringify({input_tokens:1}));return;}
        if(!url.pathname.startsWith("/v1/messages")){response.writeHead(404);response.end("fixture");return;}
        let payload;try{payload=JSON.parse(body);}catch{response.writeHead(400);response.end();return;}
        messageRequests++;
        if(messageRequests===1)firstTools=(payload.tools??[]).map(tool=>tool.name);
        const results=(payload.messages??[]).flatMap(message=>Array.isArray(message.content)?message.content:[]).filter(block=>block.type==="tool_result"&&block.tool_use_id==="fixture-image-read");
        if(results.length){toolResultObserved=true;receivedImages.push(...nestedImages(results));}
        const toolUse=messageRequests===1;
        const block=toolUse?{type:"tool_use",id:"fixture-image-read",name:"mcp__todolist__read_task_file",input:argumentsValue}:{type:"text",text:"Fixture image protocol complete."};
        if(!payload.stream){response.writeHead(200,{"Content-Type":"application/json"});response.end(JSON.stringify({id:`fixture-${messageRequests}`,type:"message",role:"assistant",model:"fixture",content:[block],stop_reason:toolUse?"tool_use":"end_turn",stop_sequence:null,usage:{input_tokens:1,output_tokens:1}}));return;}
        response.writeHead(200,{"Content-Type":"text/event-stream","Connection":"close"});
        const events=[
          ["message_start",{type:"message_start",message:{id:`fixture-${messageRequests}`,type:"message",role:"assistant",model:"fixture",content:[],stop_reason:null,stop_sequence:null,usage:{input_tokens:1,output_tokens:0}}}],
          ["content_block_start",{type:"content_block_start",index:0,content_block:toolUse?{...block,input:{}}:{type:"text",text:""}}],
          ["content_block_delta",{type:"content_block_delta",index:0,delta:toolUse?{type:"input_json_delta",partial_json:JSON.stringify(argumentsValue)}:{type:"text_delta",text:block.text}}],
          ["content_block_stop",{type:"content_block_stop",index:0}],
          ["message_delta",{type:"message_delta",delta:{stop_reason:toolUse?"tool_use":"end_turn",stop_sequence:null},usage:{output_tokens:1}}],
          ["message_stop",{type:"message_stop"}],
        ];for(const[event,data]of events)response.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);response.end();
      });
      provider.on("connect",(_request,socket)=>{blockedProxyRequests++;socket.end("HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n");});
      provider.listen(0,"127.0.0.1");await once(provider,"listening");
      env.ANTHROPIC_BASE_URL=`http://127.0.0.1:${provider.address().port}`;env.ANTHROPIC_API_KEY="fixture-only-key";
      env.HTTP_PROXY=env.ANTHROPIC_BASE_URL;env.HTTPS_PROXY=env.ANTHROPIC_BASE_URL;env.ALL_PROXY=env.ANTHROPIC_BASE_URL;
      const configuration={mcpServers:{todolist:{command:executable,args:["--client","claude_code"],env:{TODOLIST_DB_PATH:database}}}};
      try {
        const stdout=await run(claude,["--debug-file",join(root,"claude-probe-debug.log"),"--print","--output-format","stream-json","--verbose","--model","sonnet","--max-turns","2","--no-session-persistence","--strict-mcp-config","--mcp-config",JSON.stringify(configuration),"--allowedTools","mcp__todolist__read_task_file","--tools","","--","Read the isolated fixture image using its provided MCP call; then finish."],45000);
        const messages=stdout.split(/\r?\n/).filter(Boolean).flatMap(line=>{try{return[JSON.parse(line)];}catch{return[];}});
        const init=messages.find(message=>message.type==="system"&&message.subtype==="init");evidence.claude_code.discoveredReadTool=init?.tools?.includes("mcp__todolist__read_task_file")??false;
        evidence.claude_code.finalSubtype=messages.findLast(message=>message.type==="result")?.subtype;
      }catch(error){evidence.claude_code.limitation=error.message.slice(0,800);const init=error.fixtureStdout?.split(/\r?\n/).flatMap(line=>{try{return[JSON.parse(line)];}catch{return[];}}).find(message=>message.type==="system"&&message.subtype==="init");evidence.claude_code.discoveredReadTool=init?.tools?.includes("mcp__todolist__read_task_file")??false;const debug=await readFile(join(root,"claude-probe-debug.log"),"utf8").catch(()=>"");evidence.claude_code.debugTail=debug.slice(-2000).replaceAll(root,"<isolated-fixture>");}
      evidence.claude_code.providerRequests=allRequests;evidence.claude_code.messageRequests=messageRequests;
      evidence.claude_code.blockedExternalProxyRequests=blockedProxyRequests;
      evidence.claude_code.providerReadTool=firstTools.includes("mcp__todolist__read_task_file");evidence.claude_code.toolResultObserved=toolResultObserved;evidence.claude_code.imageBlocks=receivedImages;
      evidence.claude_code.verified=toolResultObserved&&receivedImages.some(image=>image.mimeType==="image/png"&&image.sha256===expectedHash);
      evidence.claude_code.transport="Installed Claude Code tool_use -> real MCP -> tool_result image in next deterministic localhost provider request; no paid inference";
      provider.closeAllConnections();await new Promise(resolve=>provider.close(resolve));provider=null;
    }
    await writeFile(reportPath,JSON.stringify(evidence,null,2)+"\n");console.log(JSON.stringify(evidence,null,2));
  }
} finally {
  if(provider){provider.closeAllConnections();await new Promise(resolve=>provider.close(resolve));}
  await Promise.allSettled(children.filter(({child})=>child.exitCode===null).map(async({child,done})=>{child.stdin.end();const timer=setTimeout(()=>child.kill(),2000);await done;clearTimeout(timer);}));
  if(!resolve(root).startsWith(resolve(tmpdir())+"\\") && !resolve(root).startsWith(resolve(tmpdir())+"/"))throw new Error("Fixture cleanup escaped temp root");
  await rm(root,{recursive:true,force:true});
}
