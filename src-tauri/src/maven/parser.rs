//! POM XML 解析器（§52）。
//!
//! 使用 `quick-xml` 流式解析，不构建完整 DOM 树（纯数据，解析完即释放）。
//! 覆盖 §52 字段全集：`groupId / artifactId / version / packaging / parent /
//! modules / dependencies / dependencyManagement / profiles / properties / plugins`。

use std::collections::BTreeMap;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::maven::model::{
    ManagedDependency, MavenDependency, MavenModule, MavenParent, MavenPlugin, MavenProfile, MavenProject,
};

/// POM 解析错误。对应全局约束 §9 的 `InvalidPom`。
#[derive(Debug, thiserror::Error)]
pub enum PomParseError {
    #[error("IO error reading pom at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("XML parse error at {path}: {source}")]
    Xml {
        path: String,
        #[source]
        source: quick_xml::Error,
    },
    #[error("Invalid pom at {path}: {reason}")]
    Invalid { path: String, reason: &'static str },
}

impl PomParseError {
    /// 该错误对应的 IPC code（§9 `InvalidPom`）。
    pub fn code(&self) -> &'static str {
        "InvalidPom"
    }
}

/// 解析给定路径的 pom.xml 文件。
pub fn parse_pom_file(path: &Path) -> Result<MavenProject, PomParseError> {
    let content = std::fs::read(path).map_err(|source| PomParseError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let file_hash = hex_hash(&content);
    parse_pom(path, &content, &file_hash)
}

/// 从已读取的字节解析 POM。
///
/// `path` 仅用于填充 `MavenProject.path`，不在此读取文件。
/// `file_hash` 由调用方提供（cache 复用同一份 hash，避免重复计算）。
pub fn parse_pom(path: &Path, content: &[u8], file_hash: &str) -> Result<MavenProject, PomParseError> {
    let path_str = path.display().to_string();
    let mut reader = Reader::from_reader(content);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut model = RawPom::default();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let event_name = e.name();
                let name = local_name(event_name.as_ref());
                if name == "project" {
                    parse_project_body(&mut reader, &mut model, &path_str)?;
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path_str,
                    source: e,
                });
            }
        }
        buf.clear();
    }

    // 校验：artifactId 必填。
    if model.artifact_id.trim().is_empty() {
        return Err(PomParseError::Invalid {
            path: path_str,
            reason: "missing <artifactId>",
        });
    }

    // groupId / version 可继承自 parent：若缺失且有 parent，留空待 effective 阶段补齐。
    Ok(MavenProject {
        path: path.to_path_buf(),
        group_id: model.group_id,
        artifact_id: model.artifact_id,
        version: model.version,
        packaging: if model.packaging.is_empty() {
            "jar".to_string()
        } else {
            model.packaging
        },
        parent: model.parent,
        modules: model.modules,
        dependencies: model.dependencies,
        dependency_management: model.dependency_management,
        profiles: model.profiles,
        properties: model.properties,
        plugins: model.plugins,
        file_hash: file_hash.to_string(),
    })
}

/// Git blob hash（cache key 用）。复用已有 git2 依赖，避免额外哈希依赖。
pub fn hex_hash(bytes: &[u8]) -> String {
    git2::Oid::hash_object(git2::ObjectType::Blob, bytes)
        .expect("hashing in-memory POM bytes cannot fail")
        .to_string()
}

fn local_name(name: &[u8]) -> &str {
    // POM XML 通常无命名空间；若有，剥离命名空间前缀。
    let s = std::str::from_utf8(name).unwrap_or("");
    match s.rfind(':') {
        Some(i) => &s[i + 1..],
        None => s,
    }
}

/// 解析过程中累积的中间结构。
#[derive(Default)]
struct RawPom {
    group_id: String,
    artifact_id: String,
    version: String,
    packaging: String,
    parent: Option<MavenParent>,
    modules: Vec<MavenModule>,
    dependencies: Vec<MavenDependency>,
    dependency_management: Vec<ManagedDependency>,
    profiles: Vec<MavenProfile>,
    properties: BTreeMap<String, String>,
    plugins: Vec<MavenPlugin>,
}

/// 解析 `<project>...</project>` 内部内容（调用方已消费 `<project>` start）。
fn parse_project_body<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    model: &mut RawPom,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let event_name = e.name();
                let name = local_name(event_name.as_ref());
                match name {
                    "parent" => {
                        let parent = parse_parent(reader, path)?;
                        model.parent = Some(parent);
                    }
                    "modules" => {
                        parse_modules(reader, &mut model.modules, path)?;
                    }
                    "dependencies" => {
                        parse_deps(reader, &mut model.dependencies, path)?;
                    }
                    "dependencyManagement" => {
                        parse_dependency_management(reader, &mut model.dependency_management, path)?;
                    }
                    "profiles" => {
                        parse_profiles(reader, &mut model.profiles, path)?;
                    }
                    "properties" => {
                        parse_properties(reader, &mut model.properties, path)?;
                    }
                    // groupId / version 可能在 parent 块内之后再次出现于 project 直接子级，
                    // 但也可能只继承自 parent。读取下一个同级标签的文本即可。
                    "groupId" => {
                        model.group_id = read_text(reader)?;
                    }
                    "artifactId" => {
                        model.artifact_id = read_text(reader)?;
                    }
                    "version" => {
                        model.version = read_text(reader)?;
                    }
                    "packaging" => {
                        model.packaging = read_text(reader)?;
                    }
                    "build" => {
                        // 解析 <build><plugins>…
                        parse_build(reader, &mut model.plugins, path)?;
                    }
                    _ => {
                        // 未知标签：跳过其整个子树。
                        skip_element(reader)?;
                    }
                }
            }
            Ok(Event::End(_)) => {
                // </project>
                break;
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

/// 读取一个标签的文本内容（消费到匹配的 End）。
fn read_text<R: std::io::BufRead>(reader: &mut Reader<R>) -> Result<String, PomParseError> {
    let mut buf = Vec::new();
    let mut out = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(t)) => {
                let decoded = t.decode().map_err(|_| PomParseError::Invalid {
                    path: String::new(),
                    reason: "invalid XML text encoding",
                })?;
                let unescaped = quick_xml::escape::unescape(&decoded).map_err(|_| PomParseError::Invalid {
                    path: String::new(),
                    reason: "invalid XML entity",
                })?;
                out.push_str(&unescaped);
            }
            Ok(Event::CData(c)) => {
                out.push_str(std::str::from_utf8(c.as_ref()).unwrap_or(""));
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: String::new(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(out.trim().to_string())
}

/// 读取标签文本，并消费一个可能存在的空标签 / 属性。`read_text` 的别名用于语义清晰。
fn read_text_opt<R: std::io::BufRead>(reader: &mut Reader<R>) -> Result<Option<String>, PomParseError> {
    let text = read_text(reader)?;
    if text.is_empty() {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

/// 跳过当前 start 标签对应的整个子树（含嵌套）。
fn skip_element<R: std::io::BufRead>(reader: &mut Reader<R>) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    let mut depth = 1;
    while depth > 0 {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => depth -= 1,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: String::new(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_parent<R: std::io::BufRead>(reader: &mut Reader<R>, path: &str) -> Result<MavenParent, PomParseError> {
    let mut group = String::new();
    let mut artifact = String::new();
    let mut version = String::new();
    let mut relative_path: Option<String> = None;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "groupId" => group = read_text(reader)?,
                "artifactId" => artifact = read_text(reader)?,
                "version" => version = read_text(reader)?,
                "relativePath" => relative_path = Some(read_text(reader)?),
                _ => skip_element(reader)?,
            },
            Ok(Event::Empty(e)) if local_name(e.name().as_ref()) == "relativePath" => {
                // `<relativePath/>` explicitly disables Maven's `../pom.xml` default.
                relative_path = Some(String::new());
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(MavenParent {
        group_id: group,
        artifact_id: artifact,
        version,
        relative_path,
    })
}

fn parse_modules<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    modules: &mut Vec<MavenModule>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == "module" => {
                let m = read_text(reader)?;
                if !m.is_empty() {
                    modules.push(MavenModule { path: m });
                }
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_dependency_management<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    managed: &mut Vec<ManagedDependency>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "dependencies" => parse_deps(reader, managed, path)?,
                _ => skip_element(reader)?,
            },
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_deps<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    deps: &mut Vec<MavenDependency>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == "dependency" => {
                let dep = parse_dependency(reader, path)?;
                deps.push(dep);
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_dependency<R: std::io::BufRead>(reader: &mut Reader<R>, path: &str) -> Result<MavenDependency, PomParseError> {
    let mut group = String::new();
    let mut artifact = String::new();
    let mut version: Option<String> = None;
    let mut scope: Option<String> = None;
    let mut optional = false;
    let mut dep_type: Option<String> = None;
    let mut classifier: Option<String> = None;
    let mut exclusions: Vec<crate::maven::model::PomCoordinates> = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "groupId" => group = read_text(reader)?,
                "artifactId" => artifact = read_text(reader)?,
                "version" => version = read_text_opt(reader)?,
                "scope" => scope = read_text_opt(reader)?,
                "optional" => optional = read_text(reader).map(|s| s == "true")?,
                "type" => dep_type = read_text_opt(reader)?,
                "classifier" => classifier = read_text_opt(reader)?,
                "exclusions" => parse_exclusions(reader, &mut exclusions, path)?,
                _ => skip_element(reader)?,
            },
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(MavenDependency {
        group_id: group,
        artifact_id: artifact,
        version,
        scope: scope
            .as_deref()
            .map(crate::maven::model::DependencyScope::parse)
            .unwrap_or_default(),
        optional,
        dep_type: dep_type.unwrap_or_else(|| "jar".to_string()),
        classifier,
        exclusions,
    })
}

fn parse_exclusions<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    exclusions: &mut Vec<crate::maven::model::PomCoordinates>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == "exclusion" => {
                let mut group = String::new();
                let mut artifact = String::new();
                let mut version = String::new();
                let mut inner = Vec::new();
                loop {
                    match reader.read_event_into(&mut inner) {
                        Ok(Event::Start(ie)) => match local_name(ie.name().as_ref()) {
                            "groupId" => group = read_text(reader)?,
                            "artifactId" => artifact = read_text(reader)?,
                            "version" => version = read_text(reader)?,
                            _ => skip_element(reader)?,
                        },
                        Ok(Event::End(_)) => break,
                        Ok(Event::Eof) => break,
                        Ok(_) => {}
                        Err(e) => {
                            return Err(PomParseError::Xml {
                                path: path.to_string(),
                                source: e,
                            });
                        }
                    }
                    inner.clear();
                }
                exclusions.push(crate::maven::model::PomCoordinates {
                    group_id: group,
                    artifact_id: artifact,
                    version,
                });
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_properties<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    props: &mut BTreeMap<String, String>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let key = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let val = read_text(reader)?;
                if !key.is_empty() {
                    props.insert(key, val);
                }
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_profiles<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    profiles: &mut Vec<MavenProfile>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == "profile" => {
                let p = parse_profile(reader, path)?;
                profiles.push(p);
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_profile<R: std::io::BufRead>(reader: &mut Reader<R>, path: &str) -> Result<MavenProfile, PomParseError> {
    let mut id = String::new();
    let mut properties = BTreeMap::new();
    let mut dependencies = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "id" => id = read_text(reader)?,
                "properties" => parse_properties(reader, &mut properties, path)?,
                "dependencies" => parse_deps(reader, &mut dependencies, path)?,
                _ => skip_element(reader)?,
            },
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(MavenProfile {
        id,
        properties,
        dependencies,
    })
}

fn parse_build<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    plugins: &mut Vec<MavenPlugin>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()) {
                "plugins" => parse_plugins(reader, plugins, path)?,
                _ => skip_element(reader)?,
            },
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

fn parse_plugins<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    plugins: &mut Vec<MavenPlugin>,
    path: &str,
) -> Result<(), PomParseError> {
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == "plugin" => {
                let mut group = String::new();
                let mut artifact = String::new();
                let mut version: Option<String> = None;
                let mut inner = Vec::new();
                loop {
                    match reader.read_event_into(&mut inner) {
                        Ok(Event::Start(ie)) => match local_name(ie.name().as_ref()) {
                            "groupId" => group = read_text(reader)?,
                            "artifactId" => artifact = read_text(reader)?,
                            "version" => version = read_text_opt(reader)?,
                            _ => skip_element(reader)?,
                        },
                        Ok(Event::End(_)) => break,
                        Ok(Event::Eof) => break,
                        Ok(_) => {}
                        Err(e) => {
                            return Err(PomParseError::Xml {
                                path: path.to_string(),
                                source: e,
                            });
                        }
                    }
                    inner.clear();
                }
                plugins.push(MavenPlugin {
                    group_id: group,
                    artifact_id: artifact,
                    version,
                });
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(PomParseError::Xml {
                    path: path.to_string(),
                    source: e,
                });
            }
        }
        buf.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>simple</artifactId>
  <version>1.0.0</version>
  <packaging>jar</packaging>
</project>"#;

    #[test]
    fn parse_simple_pom() {
        let p = std::path::Path::new("/tmp/pom.xml");
        let m = parse_pom(p, SIMPLE.as_bytes(), "deadbeef").unwrap();
        assert_eq!(m.group_id, "com.example");
        assert_eq!(m.artifact_id, "simple");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.packaging, "jar");
        assert_eq!(m.file_hash, "deadbeef");
    }

    const PARENT: &str = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>org.springframework.boot</groupId>
    <artifactId>spring-boot-starter-parent</artifactId>
    <version>3.2.0</version>
    <relativePath/>
  </parent>
  <artifactId>child</artifactId>
  <modules>
    <module>common</module>
    <module>core</module>
  </modules>
  <dependencies>
    <dependency>
      <groupId>org.projectlombok</groupId>
      <artifactId>lombok</artifactId>
      <optional>true</optional>
    </dependency>
  </dependencies>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>com.example</groupId>
        <artifactId>common</artifactId>
        <version>${project.version}</version>
      </dependency>
    </dependencies>
  </dependencyManagement>
  <properties>
    <java.version>17</java.version>
  </properties>
  <build>
    <plugins>
      <plugin>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-maven-plugin</artifactId>
      </plugin>
    </plugins>
  </build>
</project>"#;

    #[test]
    fn parse_pom_with_parent_modules_deps_props_plugins() {
        let p = std::path::Path::new("/tmp/parent.xml");
        let m = parse_pom(p, PARENT.as_bytes(), "hash").unwrap();
        assert_eq!(m.artifact_id, "child");
        assert!(m.group_id.is_empty(), "groupId inherited from parent");

        let parent = m.parent.as_ref().unwrap();
        assert_eq!(parent.group_id, "org.springframework.boot");
        assert_eq!(parent.artifact_id, "spring-boot-starter-parent");
        assert_eq!(parent.version, "3.2.0");
        assert_eq!(parent.relative_path.as_deref(), Some(""));

        assert_eq!(m.modules.len(), 2);
        assert_eq!(m.modules[0].path, "common");
        assert_eq!(m.modules[1].path, "core");

        assert_eq!(m.dependencies.len(), 1);
        let dep = &m.dependencies[0];
        assert_eq!(dep.artifact_id, "lombok");
        assert!(dep.optional);
        assert!(dep.version.is_none());

        assert_eq!(m.dependency_management.len(), 1);
        let dm = &m.dependency_management[0];
        assert_eq!(dm.version.as_deref(), Some("${project.version}"));

        assert_eq!(m.properties.get("java.version").map(|s| s.as_str()), Some("17"));

        assert_eq!(m.plugins.len(), 1);
        assert_eq!(m.plugins[0].artifact_id, "spring-boot-maven-plugin");
    }

    const PROFILES: &str = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>a</artifactId>
  <version>1</version>
  <profiles>
    <profile>
      <id>prod</id>
      <properties>
        <env>production</env>
      </properties>
      <dependencies>
        <dependency>
          <groupId>com.example</groupId>
          <artifactId>extra</artifactId>
        </dependency>
      </dependencies>
    </profile>
  </profiles>
</project>"#;

    #[test]
    fn parse_pom_with_profiles() {
        let p = std::path::Path::new("/tmp/profiles.xml");
        let m = parse_pom(p, PROFILES.as_bytes(), "h").unwrap();
        assert_eq!(m.profiles.len(), 1);
        let prof = &m.profiles[0];
        assert_eq!(prof.id, "prod");
        assert_eq!(prof.properties.get("env").map(|s| s.as_str()), Some("production"));
        assert_eq!(prof.dependencies.len(), 1);
    }

    const EXCLUSIONS: &str = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <artifactId>a</artifactId>
  <version>1</version>
  <dependencies>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter</artifactId>
      <exclusions>
        <exclusion>
          <groupId>org.springframework.boot</groupId>
          <artifactId>spring-boot-starter-logging</artifactId>
        </exclusion>
      </exclusions>
    </dependency>
  </dependencies>
</project>"#;

    #[test]
    fn parse_pom_with_exclusions() {
        let p = std::path::Path::new("/tmp/excl.xml");
        let m = parse_pom(p, EXCLUSIONS.as_bytes(), "h").unwrap();
        assert_eq!(m.dependencies.len(), 1);
        assert_eq!(m.dependencies[0].exclusions.len(), 1);
        assert_eq!(
            m.dependencies[0].exclusions[0].artifact_id,
            "spring-boot-starter-logging"
        );
    }

    #[test]
    fn missing_artifact_id_is_invalid() {
        let p = std::path::Path::new("/tmp/bad.xml");
        let bad = r#"<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>g</groupId>
  <version>1</version>
</project>"#;
        let err = parse_pom(p, bad.as_bytes(), "h").unwrap_err();
        assert_eq!(err.code(), "InvalidPom");
        assert!(matches!(err, PomParseError::Invalid { .. }));
    }

    #[test]
    fn garbage_xml_is_xml_error() {
        let p = std::path::Path::new("/tmp/bad.xml");
        let err = parse_pom(p, b"<<<not xml", "h").unwrap_err();
        assert_eq!(err.code(), "InvalidPom");
    }
}
