/** HTTP 速查静态数据（HttpStatusTool 数据源）。 */

export interface StatusEntry {
  code: number;
  name: string;
  desc: string;
}

export interface StatusGroup {
  cls: string;
  title: string;
  entries: StatusEntry[];
}

export const STATUS_GROUPS: StatusGroup[] = [
  {
    cls: "1xx",
    title: "信息响应",
    entries: [
      { code: 100, name: "Continue", desc: "客户端应继续发送请求体" },
      { code: 101, name: "Switching Protocols", desc: "协议切换（如升级到 WebSocket）" },
    ],
  },
  {
    cls: "2xx",
    title: "成功",
    entries: [
      { code: 200, name: "OK", desc: "请求成功" },
      { code: 201, name: "Created", desc: "资源已创建（常见于 POST）" },
      { code: 202, name: "Accepted", desc: "已受理但尚未处理（异步任务）" },
      { code: 204, name: "No Content", desc: "成功但无响应体（常见于 DELETE）" },
      { code: 206, name: "Partial Content", desc: "分段下载成功（Range 请求）" },
    ],
  },
  {
    cls: "3xx",
    title: "重定向",
    entries: [
      { code: 301, name: "Moved Permanently", desc: "永久重定向，搜索引擎会更新链接" },
      { code: 302, name: "Found", desc: "临时重定向（旧实现会改写成 GET）" },
      { code: 303, name: "See Other", desc: "重定向到另一个 URL，且改用 GET" },
      { code: 304, name: "Not Modified", desc: "缓存有效，直接用本地副本（协商缓存）" },
      { code: 307, name: "Temporary Redirect", desc: "临时重定向，保持原方法与请求体" },
      { code: 308, name: "Permanent Redirect", desc: "永久重定向，保持原方法与请求体" },
    ],
  },
  {
    cls: "4xx",
    title: "客户端错误",
    entries: [
      { code: 400, name: "Bad Request", desc: "请求格式错误（参数/JSON 不合法）" },
      { code: 401, name: "Unauthorized", desc: "未认证：缺少或错误的身份凭证" },
      { code: 403, name: "Forbidden", desc: "已认证但无权限访问" },
      { code: 404, name: "Not Found", desc: "资源不存在" },
      { code: 405, name: "Method Not Allowed", desc: "该路径不支持此 HTTP 方法" },
      { code: 406, name: "Not Acceptable", desc: "无法按 Accept 头提供内容" },
      { code: 408, name: "Request Timeout", desc: "服务器等请求超时" },
      { code: 409, name: "Conflict", desc: "与当前资源状态冲突（如重复创建）" },
      { code: 410, name: "Gone", desc: "资源已永久删除" },
      { code: 411, name: "Length Required", desc: "缺少 Content-Length" },
      { code: 412, name: "Precondition Failed", desc: "If-Match 等前置条件不满足" },
      { code: 413, name: "Payload Too Large", desc: "请求体超过服务器限制" },
      { code: 414, name: "URI Too Long", desc: "URL 超长" },
      { code: 415, name: "Unsupported Media Type", desc: "Content-Type 不支持" },
      { code: 416, name: "Range Not Satisfiable", desc: "Range 范围无效" },
      { code: 418, name: "I'm a teapot", desc: "彩蛋（RFC 2324，1998 愚人节）" },
      { code: 422, name: "Unprocessable Entity", desc: "格式正确但语义校验失败（常见于表单）" },
      { code: 423, name: "Locked", desc: "资源被锁定（WebDAV）" },
      { code: 426, name: "Upgrade Required", desc: "需要升级协议（如必须 HTTPS）" },
      { code: 429, name: "Too Many Requests", desc: "触发限流，稍后再试" },
      { code: 431, name: "Request Header Fields Too Large", desc: "请求头过大" },
      { code: 451, name: "Unavailable For Legal Reasons", desc: "因法律原因不可访问" },
    ],
  },
  {
    cls: "5xx",
    title: "服务器错误",
    entries: [
      { code: 500, name: "Internal Server Error", desc: "服务器内部错误（看服务端日志）" },
      { code: 501, name: "Not Implemented", desc: "服务器不支持该功能/方法" },
      { code: 502, name: "Bad Gateway", desc: "网关/代理收到上游无效响应（上游挂了）" },
      { code: 503, name: "Service Unavailable", desc: "服务暂不可用（过载或维护）" },
      { code: 504, name: "Gateway Timeout", desc: "网关等上游响应超时" },
      { code: 505, name: "HTTP Version Not Supported", desc: "不支持该 HTTP 版本" },
      { code: 507, name: "Insufficient Storage", desc: "服务器存储不足（WebDAV）" },
      { code: 511, name: "Network Authentication Required", desc: "需要网络认证（ captive portal ）" },
    ],
  },
];

export interface HeaderEntry {
  name: string;
  desc: string;
}

export const REQUEST_HEADERS: HeaderEntry[] = [
  { name: "Accept", desc: "客户端可接受的响应类型（如 application/json）" },
  { name: "Accept-Encoding", desc: "可接受的压缩算法（gzip / br / deflate）" },
  { name: "Accept-Language", desc: "可接受的语言（如 zh-CN, en;q=0.9）" },
  { name: "Authorization", desc: "身份凭证（Bearer token / Basic …）" },
  { name: "Cache-Control", desc: "缓存指令（no-cache / no-store / max-age=…）" },
  { name: "Connection", desc: "连接管理（keep-alive / close）" },
  { name: "Content-Length", desc: "请求体字节长度" },
  { name: "Content-Type", desc: "请求体类型（application/json; charset=utf-8）" },
  { name: "Cookie", desc: "携带已存储的 Cookie" },
  { name: "Host", desc: "目标主机与端口（HTTP/1.1 必需）" },
  { name: "If-Modified-Since", desc: "协商缓存：仅在此时间后修改过才返回" },
  { name: "If-None-Match", desc: "协商缓存：ETag 不匹配才返回" },
  { name: "Origin", desc: "请求来源站点（CORS 与安全校验用）" },
  { name: "Range", desc: "请求部分内容（如 bytes=0-1023，断点续传）" },
  { name: "Referer", desc: "发起请求的页面地址" },
  { name: "User-Agent", desc: "客户端标识（浏览器/设备信息）" },
  { name: "X-Forwarded-For", desc: "代理链中的原始客户端 IP" },
  { name: "X-Requested-With", desc: "标记 Ajax 请求（XMLHttpRequest）" },
];

export const RESPONSE_HEADERS: HeaderEntry[] = [
  { name: "Access-Control-Allow-Origin", desc: "CORS：允许跨域访问的来源" },
  { name: "Cache-Control", desc: "响应缓存策略（max-age / no-store / private…）" },
  { name: "Content-Disposition", desc: "attachment; filename=… 触发下载并命名" },
  { name: "Content-Encoding", desc: "响应体压缩算法（gzip / br）" },
  { name: "Content-Length", desc: "响应体字节长度" },
  { name: "Content-Type", desc: "响应体类型与字符集" },
  { name: "ETag", desc: "资源版本指纹（协商缓存/乐观锁）" },
  { name: "Expires", desc: "强缓存过期时间（旧式，优先用 Cache-Control）" },
  { name: "Last-Modified", desc: "资源最后修改时间（协商缓存）" },
  { name: "Location", desc: "重定向目标地址（配合 3xx）" },
  { name: "Retry-After", desc: "限流/维护后多久重试（秒或日期）" },
  { name: "Server", desc: "服务器软件标识（nginx / Apache…）" },
  { name: "Set-Cookie", desc: "写入 Cookie（含 HttpOnly / Secure / SameSite）" },
  { name: "Strict-Transport-Security", desc: "HSTS：强制 HTTPS 访问" },
  { name: "Vary", desc: "缓存维度（如 Accept-Encoding、Origin）" },
  { name: "WWW-Authenticate", desc: "401 时说明认证方式（Bearer realm=…）" },
  { name: "X-Content-Type-Options", desc: "nosniff：禁止 MIME 嗅探" },
  { name: "X-Frame-Options", desc: "DENY / SAMEORIGIN：防点击劫持" },
];
