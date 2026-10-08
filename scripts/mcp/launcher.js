#!/usr/bin/env node
// LightSim's MCP Bundle entry point (AI-29). The bundle carries no engine of
// its own: this finds the LightSim installed on this computer and runs its
// MCP server (`lightsim-backend mcp`), passing stdin and stdout straight
// through. If LightSim is not installed, it answers as a tiny MCP server
// whose one tool says where to download it.
"use strict";
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const DOWNLOAD = "https://github.com/Eyad-3D/simstudio/releases/latest";
const exe = process.platform === "win32" ? "lightsim-backend.exe" : "lightsim-backend";

function candidates() {
  const out = [];
  if (process.env.LIGHTSIM_ENGINE) out.push(process.env.LIGHTSIM_ENGINE);
  if (process.platform === "win32") {
    const local = process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
    out.push(path.join(local, "Programs", "LightSim", "resources", "backend", exe));
    for (const pf of [process.env.ProgramFiles, process.env["ProgramFiles(x86)"]]) {
      if (pf) out.push(path.join(pf, "LightSim", "resources", "backend", exe));
    }
  } else if (process.platform === "darwin") {
    out.push(path.join("/Applications", "LightSim.app", "Contents", "Resources", "backend", exe));
  } else {
    out.push(path.join("/opt", "LightSim", "resources", "backend", exe));
  }
  return out;
}

// arguments after this script: the folders the user allowed in the bundle's settings
const folders = process.argv.slice(2).filter((f) => f && !f.startsWith("${"));
const engine = candidates().find((p) => {
  try {
    return fs.statSync(p).isFile();
  } catch {
    return false;
  }
});

if (engine) {
  const args = ["mcp", ...folders.flatMap((f) => ["--allow-folder", f])];
  const child = spawn(engine, args, { stdio: "inherit", windowsHide: true });
  child.on("exit", (code) => process.exit(code ?? 0));
  child.on("error", (e) => {
    process.stderr.write(`LightSim could not start ${engine}: ${e.message}\n`);
    process.exit(1);
  });
} else {
  notInstalled();
}

function notInstalled() {
  const text =
    `LightSim is not installed on this computer, so the AI assistant cannot use it. ` +
    `Download and install LightSim from ${DOWNLOAD}, then restart this AI app.`;
  process.stderr.write(text + "\n");
  const send = (msg) => process.stdout.write(JSON.stringify(msg) + "\n");
  let buffer = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => {
    buffer += chunk;
    let i;
    while ((i = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, i).trim();
      buffer = buffer.slice(i + 1);
      if (!line) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        continue;
      }
      if (msg.id === undefined || !msg.method) continue;
      const info = { name: "lightsim", version: "0.0.0-not-installed" };
      let result;
      if (msg.method === "initialize") {
        result = { protocolVersion: msg.params?.protocolVersion || "2025-11-25", capabilities: { tools: {} }, serverInfo: info, instructions: text };
      } else if (msg.method === "server/discover") {
        result = { supportedVersions: ["2026-07-28"], capabilities: { tools: {} }, instructions: text, ttlMs: 0, cacheScope: "public", resultType: "complete" };
      } else if (msg.method === "tools/list") {
        result = { tools: [{ name: "lightsim_setup", description: "LightSim is not installed: says where to get it.", inputSchema: { type: "object", properties: {} } }] };
      } else if (msg.method === "tools/call") {
        result = { content: [{ type: "text", text }], isError: true };
      } else if (msg.method === "ping") {
        result = {};
      } else {
        send({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: text } });
        continue;
      }
      send({ jsonrpc: "2.0", id: msg.id, result });
    }
  });
}
