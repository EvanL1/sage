/**
 * Sage WeChat Bridge — Wechaty sidecar for Sage Daemon
 *
 * 功能：
 * 1. 接收微信消息 → 写入 events JSONL 文件 → Rust daemon 读取
 * 2. 读取 outbox JSONL 文件 → 通过微信发送 → 清空 outbox
 *
 * 运行: bun run index.ts
 */

import { WechatyBuilder, type Message } from "wechaty";
import { appendFile, readFile, writeFile } from "fs/promises";
import { existsSync, watchFile } from "fs";

const EVENTS_FILE = process.env.SAGE_EVENTS_FILE || "/tmp/sage-wechat-events.jsonl";
const OUTBOX_FILE = process.env.SAGE_OUTBOX_FILE || "/tmp/sage-wechat-outbox.jsonl";
const BOT_NAME = process.env.SAGE_BOT_NAME || "Sage";

const bot = WechatyBuilder.build({ name: BOT_NAME });

bot.on("login", (user) => {
  console.log(`[Sage WeChat] Logged in as ${user.name()}`);
});

bot.on("message", async (msg: Message) => {
  if (msg.self()) return; // 忽略自己发的消息

  const event = {
    type: "message",
    from: msg.talker().name(),
    text: msg.text(),
    room: (await msg.room())?.topic() || null,
    timestamp: new Date().toISOString(),
  };

  await appendFile(EVENTS_FILE, JSON.stringify(event) + "\n");
  console.log(`[Sage WeChat] Received from ${event.from}: ${event.text.slice(0, 50)}`);
});

// 监听 outbox 文件，有新消息时发送
watchFile(OUTBOX_FILE, { interval: 2000 }, async () => {
  if (!existsSync(OUTBOX_FILE)) return;

  const content = await readFile(OUTBOX_FILE, "utf-8");
  if (!content.trim()) return;

  for (const line of content.split("\n")) {
    if (!line.trim()) continue;
    try {
      const msg = JSON.parse(line);
      const contact = await bot.Contact.find({ name: msg.to });
      if (contact) {
        await contact.say(msg.text);
        console.log(`[Sage WeChat] Sent to ${msg.to}: ${msg.text.slice(0, 50)}`);
      }
    } catch (e) {
      console.error(`[Sage WeChat] Send failed:`, e);
    }
  }

  // 清空 outbox
  await writeFile(OUTBOX_FILE, "");
});

bot.start().catch(console.error);
console.log(`[Sage WeChat] Bridge starting... events → ${EVENTS_FILE}`);
