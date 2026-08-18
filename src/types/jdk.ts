/**
 * R-04 JDK Manager IPC types.
 * Source of truth: Rust java/model.rs (JdkInstallation / JdkDiscoverySource / JdkVendor).
 * serde uses camelCase, so TS fields are camelCase to match.
 */

/** JDK 发现来源（§31）。与 Rust JdkDiscoverySource camelCase 序列化对齐。 */
export type JdkDiscoverySource =
  | "system"
  | "javaHome"
  | "path"
  | "mise"
  | "jenv"
  | "sdkman"
  | "manual";

/** JDK 厂商（§32）。与 Rust JdkVendor camelCase 序列化对齐。 */
export type JdkVendor =
  | "oracle"
  | "openJdk"
  | "temurin"
  | "corretto"
  | "graalVm"
  | "zulu"
  | "liberica"
  | "other";

/** 一个已发现 / 注册的 JDK 安装。 */
export interface JdkInstallation {
  id?: number;
  homePath: string;
  majorVersion?: number;
  fullVersion?: string;
  vendor?: JdkVendor;
  architecture?: string;
  bitness?: number;
  source: JdkDiscoverySource;
  javaExec?: string;
  javacExec?: string;
  isValid: boolean;
  lastChecked: string;
  rawVersion?: string;
  createdAt?: string;
  updatedAt?: string;
}
