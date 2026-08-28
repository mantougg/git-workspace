/**
 * F-04：新建应用的启动预设。新增预设只需在数组里加条目，不改其他代码。
 *
 * vmOptions 逐行写入向导的 VM Options（每行一个 JVM 参数）。
 */
export interface LaunchPreset {
  id: string;
  label: string;
  description: string;
  vmOptions: string[];
}

/**
 * 对齐 IntelliJ IDEA 启动 Spring Boot 应用的 JVM 参数。
 *
 * 刻意排除的 IDEA 私有项：
 * - `-javaagent:.../idea_rt.jar=<port>`：IDEA 调试器代理，没有 IDEA 在
 *   监听时无意义（且路径随本机 IDEA 安装变）。
 * - `@<temp>/idea_arg_file*`：IDEA 的命令行参数文件，由 IDEA 动态生成。
 * 主类与 JDK 由应用配置本身承担，不进预设。
 */
export const LAUNCH_PRESETS: LaunchPreset[] = [
  {
    id: "idea-spring-boot",
    label: "IDEA 启动（Spring Boot）",
    description:
      "对齐 IDEA 启动 Spring Boot 的 JVM 参数（不含 idea_rt.jar / @arg_file 等 IDEA 私有项）",
    vmOptions: [
      "-XX:TieredStopAtLevel=1",
      "-Dspring.output.ansi.enabled=always",
      "-Dcom.sun.management.jmxremote",
      "-Dspring.jmx.enabled=true",
      "-Dspring.liveBeansView.mbeanDomain",
      "-Dspring.application.admin.enabled=true",
      "-Dmanagement.endpoints.jmx.exposure.include=*",
      "-Dfile.encoding=UTF-8",
    ],
  },
];
