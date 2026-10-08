use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use quick_xml::{events::Event, Reader};
use tokio_util::sync::CancellationToken;
use zip::{CompressionMethod, ZipArchive};

use super::{check, ExportFailure, RenderedBook, Result};

fn invalid(message: impl Into<String>) -> ExportFailure {
    ExportFailure::Validation {
        message: message.into(),
    }
}

#[derive(Default)]
struct XmlInfo {
    ids: HashSet<String>,
    links: Vec<String>,
    manifest: HashMap<String, (String, String, String)>,
    spine: Vec<String>,
    rootfile: Option<String>,
    toc: bool,
}

fn xml(ct: &CancellationToken, input: impl BufRead, path: &str) -> Result<XmlInfo> {
    let mut reader = Reader::from_reader(input);
    reader.config_mut().check_comments = true;
    let mut buffer = Vec::new();
    let mut info = XmlInfo::default();
    let mut depth = 0usize;
    let mut roots = 0;
    loop {
        check(ct)?;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|e| invalid(format!("{path}: {e}")))?;
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                if depth == 0 {
                    roots += 1;
                }
                let mut attrs = HashMap::new();
                for attr in element.attributes() {
                    let attr = attr.map_err(|e| invalid(format!("{path}: {e}")))?;
                    let key = std::str::from_utf8(attr.key.as_ref())
                        .map_err(|e| invalid(e.to_string()))?
                        .to_owned();
                    let value = attr
                        .decode_and_unescape_value(reader.decoder())
                        .map_err(|e| invalid(format!("{path}: {e}")))?
                        .into_owned();
                    attrs.insert(key, value);
                }
                if let Some(id) = attrs.get("id") {
                    if !info.ids.insert(id.clone()) {
                        return Err(invalid(format!("duplicate ID {id} in {path}")));
                    }
                }
                if let Some(href) = attrs.get("href") {
                    info.links.push(href.clone());
                }
                let get = |key: &str| attrs.get(key).cloned().unwrap_or_default();
                if depth == 0 {
                    let expected = if path.ends_with(".xhtml") {
                        ("html", "http://www.w3.org/1999/xhtml")
                    } else if path.ends_with(".opf") {
                        ("package", "http://www.idpf.org/2007/opf")
                    } else {
                        (
                            "container",
                            "urn:oasis:names:tc:opendocument:xmlns:container",
                        )
                    };
                    if element.name().as_ref() != expected.0.as_bytes()
                        || get("xmlns") != expected.1
                    {
                        return Err(invalid(format!("invalid XML root in {path}")));
                    }
                }
                match element.local_name().as_ref() {
                    b"item" => {
                        info.manifest.insert(
                            get("id"),
                            (get("href"), get("media-type"), get("properties")),
                        );
                    }
                    b"itemref" => info.spine.push(get("idref")),
                    b"rootfile" => info.rootfile = Some(get("full-path")),
                    b"nav" if get("epub:type") == "toc" => info.toc = true,
                    _ => {}
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid(format!("unbalanced XML in {path}")))?;
            }
            Event::Text(text) => {
                let text = text
                    .xml_content()
                    .map_err(|e| invalid(format!("{path}: {e}")))?;
                super::plan::xml_text(ct, &text, path)?;
                if depth == 0 && !text.trim().is_empty() {
                    return Err(invalid(format!("text outside root in {path}")));
                }
            }
            Event::GeneralRef(reference) => {
                let name = reference.decode().map_err(|e| invalid(e.to_string()))?;
                if !matches!(name.as_ref(), "amp" | "lt" | "gt" | "quot" | "apos") {
                    let ch = reference
                        .resolve_char_ref()
                        .map_err(|e| invalid(e.to_string()))?
                        .ok_or_else(|| invalid(format!("unknown entity {name} in {path}")))?;
                    super::plan::xml_text(ct, &ch.to_string(), path)?;
                }
            }
            Event::DocType(_) => return Err(invalid(format!("unexpected DTD in {path}"))),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if depth != 0 || roots != 1 {
        return Err(invalid(format!("incomplete XML in {path}")));
    }
    Ok(info)
}

fn resolve(path: &str, href: &str) -> Result<(String, Option<String>)> {
    let (relative, fragment) = href
        .split_once('#')
        .map_or((href, None), |(p, f)| (p, Some(f.to_owned())));
    if relative.contains([':', '\\', '?', '%']) || relative.starts_with('/') {
        return Err(invalid(format!("nonlocal resource link {href} in {path}")));
    }
    let mut parts: Vec<_> = path.split('/').collect();
    if !relative.is_empty() {
        parts.pop();
        for part in relative.split('/') {
            match part {
                ".." => {
                    parts
                        .pop()
                        .ok_or_else(|| invalid("resource path escapes package"))?;
                }
                "." | "" => {}
                part => parts.push(part),
            }
        }
    }
    Ok((parts.join("/"), fragment))
}

pub(super) fn epub(ct: &CancellationToken, path: &Path, book: &RenderedBook) -> Result<()> {
    let mut zip = ZipArchive::new(File::open(path)?)?;
    let mut resources = HashSet::new();
    let mut xml_files = HashMap::new();
    for index in 0..zip.len() {
        check(ct)?;
        let mut entry = zip.by_index(index)?;
        let name = entry.name().to_owned();
        if !resources.insert(name.clone()) {
            return Err(invalid(format!("duplicate resource {name}")));
        }
        if index == 0 {
            if name != "mimetype"
                || entry.compression() != CompressionMethod::Stored
                || entry.size() != 20
                || entry.encrypted()
                || entry.extra_data().is_some_and(|d| !d.is_empty())
            {
                return Err(invalid("invalid EPUB mimetype entry"));
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            if bytes != b"application/epub+zip" {
                return Err(invalid("invalid EPUB mimetype content"));
            }
        } else if name.ends_with(".xhtml") || name.ends_with(".opf") || name.ends_with(".xml") {
            xml_files.insert(name.clone(), xml(ct, BufReader::new(entry), &name)?);
        } else {
            std::io::copy(
                &mut entry,
                &mut super::CancelWriter {
                    ct,
                    inner: std::io::sink(),
                },
            )?;
        }
    }
    let container = xml_files
        .get("META-INF/container.xml")
        .ok_or_else(|| invalid("missing container"))?;
    if container.rootfile.as_deref() != Some("EPUB/package.opf") {
        return Err(invalid("invalid container rootfile"));
    }
    let package = xml_files
        .get("EPUB/package.opf")
        .ok_or_else(|| invalid("missing package"))?;
    let navigation = xml_files
        .get("EPUB/nav.xhtml")
        .ok_or_else(|| invalid("missing navigation"))?;
    if !navigation.toc || navigation.links.is_empty() {
        return Err(invalid("missing TOC navigation"));
    }
    let expected: Vec<_> = book
        .files
        .iter()
        .enumerate()
        .map(|(i, _)| format!("content-{}", i + 1))
        .collect();
    let (cover_items, cover_spine) = if book.cover { (2, 1) } else { (0, 0) };
    if package.spine[cover_spine.min(package.spine.len())..] != expected
        || package.spine.len() != expected.len() + cover_spine
        || package.manifest.len() != book.files.len() + 2 + cover_items
    {
        return Err(invalid("manifest or spine differs from the rendered book"));
    }
    if book.cover
        && (package.spine.first().map(String::as_str) != Some("cover")
            || package.manifest.get("cover")
                != Some(&(
                    super::render::COVER_PAGE.into(),
                    "application/xhtml+xml".into(),
                    String::new(),
                ))
            || package.manifest.get("cover-image")
                != Some(&(
                    super::render::COVER_IMAGE.into(),
                    "image/jpeg".into(),
                    "cover-image".into(),
                )))
    {
        return Err(invalid("invalid cover manifest item"));
    }
    for (index, file) in book.files.iter().enumerate() {
        if package.manifest.get(&expected[index])
            != Some(&(file.clone(), "application/xhtml+xml".into(), String::new()))
        {
            return Err(invalid(format!("invalid manifest item for {file}")));
        }
    }
    if package.manifest.get("nav")
        != Some(&(
            "nav.xhtml".into(),
            "application/xhtml+xml".into(),
            "nav".into(),
        ))
        || package.manifest.get("css")
            != Some(&("styles/book.css".into(), "text/css".into(), String::new()))
    {
        return Err(invalid("invalid navigation or stylesheet manifest item"));
    }
    for (path, info) in &xml_files {
        check(ct)?;
        for href in &info.links {
            let (target, fragment) = resolve(path, href)?;
            if !resources.contains(&target)
                || fragment.is_some_and(|id| {
                    !xml_files
                        .get(&target)
                        .is_some_and(|info| info.ids.contains(&id))
                })
            {
                return Err(invalid(format!("broken link {href} in {path}")));
            }
        }
    }
    check(ct)
}
