/**
 * 从 build/icon.png 生成 Windows 安装包所需的 256×256 icon.ico。
 *
 * ICO 允许直接嵌入 PNG（Vista+）。这里只放一个 256×256 条目，
 * 由 Windows 在需要小尺寸时缩放；比维护多尺寸手工图标更不容易漂移。
 */

import fs from "node:fs";
import path from "node:path";

const appDir = path.resolve(import.meta.dirname, "..");
const pngPath = path.join(appDir, "build", "icon.png");
const icoPath = path.join(appDir, "build", "icon.ico");

const png = fs.readFileSync(pngPath);
const signature = png.subarray(0, 8).toString("hex");
if (signature !== "89504e470d0a1a0a") {
  throw new Error(`${pngPath} is not a PNG file`);
}
const width = png.readUInt32BE(16);
const height = png.readUInt32BE(20);
if (width !== 256 || height !== 256) {
  throw new Error(`icon.png must be 256x256, got ${width}x${height}`);
}

const header = Buffer.alloc(6 + 16);
header.writeUInt16LE(0, 0); // reserved
header.writeUInt16LE(1, 2); // type: icon
header.writeUInt16LE(1, 4); // one image
header[6] = 0; // width 256 is encoded as 0
header[7] = 0; // height 256 is encoded as 0
header[8] = 0; // palette size
header[9] = 0; // reserved
header.writeUInt16LE(1, 10); // color planes
header.writeUInt16LE(32, 12); // bits per pixel
header.writeUInt32LE(png.length, 14);
header.writeUInt32LE(header.length, 18);

fs.writeFileSync(icoPath, Buffer.concat([header, png]));
console.log(`wrote ${icoPath} (256x256, ${png.length + header.length} bytes)`);
