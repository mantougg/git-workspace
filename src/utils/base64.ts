/**
 * 分块 base64 编码工具。
 *
 * `btoa(String.fromCharCode(...bytes))` 的 spread 写法在输入几万字节以上时
 * 超出引擎参数上限，抛 RangeError（Maximum call stack size exceeded）。
 * 此处分块（32KB）编码后拼接，与 Base64Tool 内联实现语义一致。
 */

const CHUNK = 0x8000;

export function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

/** UTF-8 文本 → base64（btoa 只认 Latin-1，多字节字符先经 TextEncoder）。 */
export function encodeUtf8Base64(text: string): string {
  return bytesToBase64(new TextEncoder().encode(text));
}
