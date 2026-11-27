import React from "react";
import { createRoot } from "react-dom/client";
import { SoftwareUpdatePanel } from "/src/SoftwareUpdateCard.tsx";
import "/src/styles.css";
const noNetwork = async () => {};
const now = Date.now();
createRoot(document.getElementById("root")!).render(<main style={{maxWidth:740,margin:"48px auto",padding:24}}>
  <h1 style={{fontSize:24}}>合成限流状态</h1><p style={{color:"#64748b",marginBottom:24}}>仅验证本地界面；无更新源请求，无安装操作。</p>
  <SoftwareUpdatePanel update={{desktop:true,currentVersion:"0.2.23",availableVersion:null,phase:"error",message:"GitHub 请求已限流，请在重试时间后再次检查。",progress:null,componentStatus:null,recovery:null,lastAttemptAt:now,lastCheckedAt:now-24*3600000,nextAutomaticCheckAt:now+6*3600000,nextManualCheckAt:now+60000,retryAt:now+3600000,checkBlockedUntil:now+3600000,lastCheckResult:"error",checkForUpdate:noNetwork,checkIfStale:noNetwork,downloadAndInstall:async()=>false,retryRetainedUpdate:async()=>false,discardRetainedUpdate:async()=>false,openRetainedUpdateLocation:async()=>false}} />
</main>);
