//! Bounded, read-only extraction of managed attachments. No parser evaluates document code.

use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read},
    path::Path,
};

use image::{ImageDecoder, ImageFormat, ImageReader};
use quick_xml::{events::Event, Reader};

pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const XML_BYTES: usize = 8 * 1024 * 1024;
const EXPANDED_BYTES: u64 = 32 * 1024 * 1024;
const TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_UNITS: usize = 100_000;
const MAX_OUTPUT: usize = 256 * 1024;
const PDF_STREAM_BYTES: usize = 1024 * 1024;
const PDF_DECODED_BYTES: usize = 32 * 1024 * 1024;
const PDF_FORM_DEPTH: usize = 16;
const PDF_XOBJECT_CALLS: usize = 256;
const PDF_OPERATIONS: usize = 200_000;

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub start: usize,
    pub offset: usize,
    pub limit: usize,
    pub max_bytes: usize,
    pub sheet: Option<String>,
    pub range: Option<String>,
    pub original: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            start: 0,
            offset: 0,
            limit: 20,
            max_bytes: 16 * 1024,
            sheet: None,
            range: None,
            original: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContentUnit {
    pub location: String,
    pub text: String,
}

#[derive(Debug)]
pub struct ImagePayload {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[derive(Debug)]
pub struct ReadOutput {
    pub format: String,
    pub status: String,
    pub units: Vec<ContentUnit>,
    pub next_start: Option<usize>,
    pub next_offset: usize,
    pub truncated: bool,
    pub image: Option<ImagePayload>,
    pub message: Option<String>,
}

#[derive(Debug)]
struct Failure {
    status: &'static str,
    message: String,
}
type ExtractResult<T> = Result<T, Failure>;

fn fail(status: &'static str, message: impl Into<String>) -> Failure {
    Failure {
        status,
        message: message.into(),
    }
}

fn output(format: &str, status: &str, message: Option<String>) -> ReadOutput {
    ReadOutput {
        format: format.into(),
        status: status.into(),
        units: vec![],
        next_start: None,
        next_offset: 0,
        truncated: false,
        image: None,
        message,
    }
}

/// `start` is a zero-based unit index; `offset` is a UTF-8 byte offset in that unit.
/// Callers bind these positions to a file hash so re-parsing has deterministic continuation.
pub fn read_bytes(
    bytes: &[u8],
    original_name: &str,
    options: &ReadOptions,
) -> Result<ReadOutput, String> {
    if options.limit == 0 || options.limit > 200 || !(128..=MAX_OUTPUT).contains(&options.max_bytes)
    {
        return Err("limit must be 1..200 and max_bytes must be 128..262144".into());
    }
    let extension = Path::new(original_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let format = match extension.as_str() {
        "pdf" | "docx" | "pptx" | "xlsx" => extension.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => "image",
        "doc" | "ppt" | "xls" => return Ok(output(
            &extension,
            "unsupported",
            Some(
                "Legacy binary Office documents are unsupported; save a DOCX, PPTX or XLSX copy."
                    .into(),
            ),
        )),
        "txt" | "md" | "markdown" | "csv" | "tsv" | "json" | "jsonl" | "xml" | "html" | "htm"
        | "log" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "rs" | "js" | "jsx" | "ts" | "tsx"
        | "css" | "py" | "sql" | "sh" | "ps1" | "bat" | "c" | "cpp" | "h" | "hpp" => "text",
        _ => {
            return Ok(output(
                &extension,
                "unsupported",
                Some("This file type is not supported for content extraction.".into()),
            ))
        }
    };
    if format != "xlsx" && (options.sheet.is_some() || options.range.is_some()) {
        return Err("sheet and range apply only to XLSX files".into());
    }
    if format != "image" && options.original {
        return Err("original applies only to image files".into());
    }
    if bytes.len() > MAX_INPUT_BYTES {
        return Ok(output(
            format,
            "resource_limit",
            Some("File exceeds the 16 MiB parser input limit.".into()),
        ));
    }
    let extracted = match format {
        "image" => image_content(&bytes, options),
        "pdf" => pdf_content(&bytes, options),
        "text" => text_content(&bytes).and_then(|units| paginate(format, units, options)),
        _ => office_content(&bytes, format, options).and_then(|units| {
            let mut result = paginate(format, units, options)?;
            if format == "xlsx" { result.message = Some("Cells expose stored values; number/date formatting and formulas are not evaluated.".into()); }
            Ok(result)
        }),
    };
    Ok(match extracted {
        Ok(value) => value,
        Err(error) => output(format, error.status, Some(error.message)),
    })
}

fn paginate(
    format: &str,
    units: Vec<ContentUnit>,
    options: &ReadOptions,
) -> ExtractResult<ReadOutput> {
    if options.start > units.len() || (options.start == units.len() && options.offset != 0) {
        return Err(fail(
            "invalid_position",
            "Content position is outside the document.",
        ));
    }
    let mut result = output(format, "ok", None);
    let mut used = 0;
    for (index, unit) in units.into_iter().enumerate().skip(options.start) {
        let offset = if index == options.start {
            options.offset
        } else {
            0
        };
        if offset > unit.text.len() || !unit.text.is_char_boundary(offset) {
            return Err(fail(
                "invalid_position",
                "Content offset is not a valid UTF-8 boundary.",
            ));
        }
        if result.units.len() == options.limit || used == options.max_bytes {
            result.next_start = Some(index);
            result.truncated = true;
            break;
        }
        let remaining = &unit.text[offset..];
        let mut end = remaining.len().min(options.max_bytes - used);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 && !remaining.is_empty() {
            result.next_start = Some(index);
            result.next_offset = offset;
            result.truncated = true;
            break;
        }
        used += end;
        result.units.push(ContentUnit {
            location: unit.location,
            text: remaining[..end].into(),
        });
        if end < remaining.len() {
            result.next_start = Some(index);
            result.next_offset = offset + end;
            result.truncated = true;
            break;
        }
    }
    Ok(result)
}

fn strict_text(bytes: &[u8]) -> ExtractResult<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err(fail(
                "invalid_encoding",
                "UTF-16 text has an incomplete code unit.",
            ));
        }
        let little = bytes[0] == 0xff;
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&words).map_err(|_| {
            fail(
                "invalid_encoding",
                "UTF-16 text contains invalid surrogate pairs.",
            )
        })
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|_| {
            fail(
                "invalid_encoding",
                "Text must be valid UTF-8, or BOM-marked UTF-16.",
            )
        })
    }
}

fn text_content(bytes: &[u8]) -> ExtractResult<Vec<ContentUnit>> {
    let text = strict_text(bytes)?;
    if text.contains('\0') {
        return Err(fail("invalid_encoding", "Text contains binary NUL bytes."));
    }
    let mut units = Vec::new();
    let mut total = 0;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        push_unit(
            &mut units,
            &mut total,
            format!("line:{}", index + 1),
            line.into(),
        )?;
    }
    Ok(units)
}

fn push_unit(
    units: &mut Vec<ContentUnit>,
    total: &mut usize,
    location: String,
    text: String,
) -> ExtractResult<()> {
    *total = total
        .checked_add(text.len())
        .ok_or_else(|| fail("resource_limit", "Extracted text exceeds its limit."))?;
    if *total > TEXT_BYTES || units.len() >= MAX_UNITS {
        return Err(fail(
            "resource_limit",
            "Extracted text exceeds the 16 MiB / 100000 unit limit.",
        ));
    }
    units.push(ContentUnit { location, text });
    Ok(())
}

fn image_content(bytes: &[u8], options: &ReadOptions) -> ExtractResult<ReadOutput> {
    if options.start != 0 || options.offset != 0 {
        return Err(fail(
            "invalid_position",
            "Images have no text continuation position.",
        ));
    }
    let format =
        image::guess_format(bytes).map_err(|_| fail("corrupt", "Image header is invalid."))?;
    if !matches!(
        format,
        ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::WebP
            | ImageFormat::Bmp
    ) {
        return Err(fail("unsupported", "Image encoding is unsupported."));
    }
    if format == ImageFormat::WebP {
        preflight_webp(bytes)?;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(image_failure)?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(fail("resource_limit", "Image exceeds 16 million pixels."));
    }
    let image = image::DynamicImage::from_decoder(decoder).map_err(image_failure)?;
    let mut result = output(
        "image",
        "requires_ocr",
        Some(format!(
            "{}x{} image; OCR is not performed. {}",
            width,
            height,
            if options.original {
                "Original bytes returned after bounded validation."
            } else {
                "A bounded PNG thumbnail is returned (first frame for animations)."
            }
        )),
    );
    let (data, mime) = if options.original {
        if bytes.len() > options.max_bytes {
            return Err(fail("resource_limit", "Original image exceeds max_bytes; request a thumbnail or raise max_bytes within the output limit."));
        }
        (bytes.to_vec(), format.to_mime_type().to_string())
    } else {
        let mut edge = 1024u32;
        let encoded = loop {
            let preview = image.thumbnail(edge, edge);
            let mut encoded = Cursor::new(Vec::new());
            preview
                .write_to(&mut encoded, ImageFormat::Png)
                .map_err(|_| fail("corrupt", "Image thumbnail could not be encoded."))?;
            if encoded.get_ref().len() <= options.max_bytes {
                break encoded.into_inner();
            }
            if edge == 1 {
                return Err(fail(
                    "resource_limit",
                    "Image cannot fit the output byte budget.",
                ));
            }
            edge = (edge / 2).max(1);
        };
        (encoded, "image/png".into())
    };
    result.image = Some(ImagePayload {
        bytes: data,
        mime_type: mime,
    });
    Ok(result)
}

fn image_failure(error: image::ImageError) -> Failure {
    if matches!(error, image::ImageError::Limits(_)) {
        fail(
            "resource_limit",
            "Image exceeds its dimension or 64 MiB allocation limit.",
        )
    } else {
        fail("corrupt", "Image could not be decoded.")
    }
}

fn preflight_webp(bytes: &[u8]) -> ExtractResult<()> {
    fn dimensions(width: u32, height: u32) -> ExtractResult<()> {
        if width == 0 || height == 0 {
            return Err(fail("corrupt", "WebP has an empty frame."));
        }
        if width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 16_000_000 {
            return Err(fail(
                "resource_limit",
                "WebP canvas or embedded frame exceeds its dimension/pixel limit.",
            ));
        }
        Ok(())
    }
    fn u24(bytes: &[u8]) -> u32 {
        u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16
    }
    fn chunks(
        data: &[u8],
        nested: bool,
        count: &mut usize,
        frames: &mut usize,
    ) -> ExtractResult<()> {
        let mut index = 0usize;
        while index < data.len() {
            *count += 1;
            if *count > 4096 {
                return Err(fail("resource_limit", "WebP exceeds 4096 chunks."));
            }
            let header = data
                .get(index..index + 8)
                .ok_or_else(|| fail("corrupt", "WebP chunk header is truncated."))?;
            let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let end = index
                .checked_add(8)
                .and_then(|v| v.checked_add(length))
                .ok_or_else(|| fail("corrupt", "WebP chunk length overflows."))?;
            let payload = data
                .get(index + 8..end)
                .ok_or_else(|| fail("corrupt", "WebP chunk payload is truncated."))?;
            match &header[..4] {
                b"VP8 " => {
                    if payload.len() < 10 || payload[3..6] != [0x9d, 0x01, 0x2a] {
                        return Err(fail("corrupt", "WebP VP8 frame header is invalid."));
                    }
                    dimensions(
                        u32::from(u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff),
                        u32::from(u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff),
                    )?;
                }
                b"VP8L" => {
                    if payload.len() < 5 || payload[0] != 0x2f {
                        return Err(fail("corrupt", "WebP lossless frame header is invalid."));
                    }
                    let bits = u32::from_le_bytes(payload[1..5].try_into().unwrap());
                    dimensions((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)?;
                }
                b"VP8X" => {
                    if payload.len() != 10 || nested {
                        return Err(fail("corrupt", "WebP canvas header is invalid."));
                    }
                    dimensions(u24(&payload[4..7]) + 1, u24(&payload[7..10]) + 1)?;
                }
                b"ANMF" => {
                    *frames += 1;
                    if *frames > 128 {
                        return Err(fail("resource_limit", "WebP exceeds 128 animation frames."));
                    }
                    if payload.len() < 16 || nested {
                        return Err(fail("corrupt", "WebP animation frame header is invalid."));
                    }
                    dimensions(u24(&payload[6..9]) + 1, u24(&payload[9..12]) + 1)?;
                    chunks(&payload[16..], true, count, frames)?;
                }
                _ => {}
            }
            index = end
                .checked_add(length % 2)
                .ok_or_else(|| fail("corrupt", "WebP padding length overflows."))?;
            if index > data.len() {
                return Err(fail("corrupt", "WebP chunk padding is missing."));
            }
        }
        Ok(())
    }
    if bytes.len() < 12 {
        return Err(fail("corrupt", "WebP RIFF header is truncated."));
    }
    let length = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let end = length
        .checked_add(8)
        .ok_or_else(|| fail("corrupt", "WebP RIFF length overflows."))?;
    let data = bytes
        .get(12..end)
        .ok_or_else(|| fail("corrupt", "WebP RIFF data is truncated."))?;
    chunks(data, false, &mut 0, &mut 0)
}

fn pdf_content(bytes: &[u8], options: &ReadOptions) -> ExtractResult<ReadOutput> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(fail("corrupt", "PDF header is invalid."));
    }
    // Each eagerly decoded xref/object stream also has a per-stream cap. Bound the
    // number of possible stream tokens before the PDF parser allocates objects.
    if bytes.windows(6).filter(|w| *w == b"stream").count() > 512 {
        return Err(fail(
            "resource_limit",
            "PDF exceeds the bounded stream count.",
        ));
    }
    let document = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            strict: true,
            max_decompressed_size: Some(PDF_STREAM_BYTES),
            ..Default::default()
        },
    )
    .map_err(pdf_failure)?;
    if document.is_encrypted() || document.was_encrypted() {
        return Err(fail("encrypted", "Encrypted PDFs are unsupported."));
    }
    if document.objects.len() > 50_000 {
        return Err(fail("resource_limit", "PDF has too many objects."));
    }
    preflight_pdf_fonts(&document)?;
    let pages = document.get_pages();
    if pages.is_empty() {
        return Err(fail("corrupt", "PDF contains no pages."));
    }
    if pages.len() > 2000 {
        return Err(fail("resource_limit", "PDF exceeds 2000 pages."));
    }
    if options.start > pages.len() {
        return Err(fail(
            "invalid_position",
            "Page position is outside the PDF.",
        ));
    }
    let mut units = Vec::with_capacity(pages.len());
    let mut total = 0;
    let mut empty = 0;
    let mut budget = PdfBudget::default();
    let mut needs_ocr = false;
    let mut incomplete = false;
    // Only pages that can occur in this response are decoded. Placeholders retain
    // absolute page indices; pagination can resume inside a single large page.
    for (index, page) in pages.keys().enumerate() {
        let text = if index >= options.start && index < options.start.saturating_add(options.limit)
        {
            let extracted = bounded_pdf_page(&document, pages[page], &mut budget)?;
            needs_ocr |= extracted.needs_ocr;
            incomplete |= extracted.incomplete;
            if extracted.text.trim().is_empty() {
                empty += 1;
            }
            extracted.text
        } else {
            String::new()
        };
        push_unit(&mut units, &mut total, format!("page:{}", page), text)?;
    }
    let mut result = paginate("pdf", units, options)?;
    if empty > 0 || needs_ocr || incomplete {
        result.status = if result.units.iter().all(|u| u.text.trim().is_empty()) {
            "requires_ocr"
        } else {
            "partial"
        }
        .into();
        result.message = Some(if needs_ocr || empty > 0 {
            "Selected PDF content includes images or pages without extractable text; OCR is not performed. Available page/Form text is preserved; text search cannot cover the omitted visual content."
        } else {
            "Some selected PDF content uses unresolved or unsupported resources; available page/Form text is preserved, but extraction is incomplete."
        }.into());
    }
    Ok(result)
}

fn pdf_failure(error: lopdf::Error) -> Failure {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. })
        | lopdf::Error::ReferenceLimit
        | lopdf::Error::RecursionLimit => fail(
            "resource_limit",
            "PDF exceeds its stream, font or object traversal limits.",
        ),
        lopdf::Error::InvalidPassword
        | lopdf::Error::Decryption(_)
        | lopdf::Error::AlreadyEncrypted => fail("encrypted", "Encrypted PDFs are unsupported."),
        _ => fail("corrupt", "PDF page text could not be decoded."),
    }
}

fn preflight_pdf_fonts(document: &lopdf::Document) -> ExtractResult<()> {
    let mut stack = document.objects.values().collect::<Vec<_>>();
    let mut checked = HashSet::new();
    let mut cmap_bytes = 0usize;
    let mut cmap_entries = 0u64;
    while let Some(object) = stack.pop() {
        let dictionary = match object {
            lopdf::Object::Dictionary(dictionary) => Some(dictionary),
            lopdf::Object::Stream(stream) => Some(&stream.dict),
            lopdf::Object::Array(items) => {
                stack.extend(items);
                None
            }
            _ => None,
        };
        if let Some(dictionary) = dictionary {
            if let Ok(lopdf::Object::Reference(id)) = dictionary.get(b"ToUnicode") {
                if checked.insert(*id) {
                    let stream = document
                        .get_object(*id)
                        .and_then(lopdf::Object::as_stream)
                        .map_err(pdf_failure)?;
                    let data = stream
                        .get_plain_content_with_limit(PDF_STREAM_BYTES)
                        .map_err(pdf_failure)?;
                    cmap_bytes += data.len();
                    if cmap_bytes > 8 * 1024 * 1024 || checked.len() > 64 {
                        return Err(fail(
                            "resource_limit",
                            "PDF font maps exceed their combined limit.",
                        ));
                    }
                    cmap_entries += preflight_cmap(&data)?;
                    if cmap_entries > 65_536 {
                        return Err(fail(
                            "resource_limit",
                            "PDF font maps exceed 65536 mapped characters.",
                        ));
                    }
                }
            }
            stack.extend(dictionary.iter().map(|(_, value)| value));
        }
    }
    Ok(())
}

// lopdf's font map builds a reverse index over every bfrange value. Validate
// expansion before that constructor runs; compressed byte limits alone cannot
// stop a tiny `<00000000> <FFFFFFFF>` range allocating billions of entries.
fn preflight_cmap(data: &[u8]) -> ExtractResult<u64> {
    #[derive(Debug)]
    enum Token {
        Hex(Vec<u8>),
        Word(String),
        Open,
        Close,
    }
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < data.len() {
        let byte = data[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if byte == b'%' {
            while index < data.len() && data[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if data[index..].starts_with(b"<<") || data[index..].starts_with(b">>") {
            index += 2;
        } else if byte == b'<' {
            index += 1;
            let mut digits = Vec::new();
            while index < data.len() && data[index] != b'>' {
                if !data[index].is_ascii_whitespace() {
                    digits.push(data[index]);
                }
                index += 1;
            }
            if index == data.len() || digits.len() % 2 != 0 {
                return Err(fail(
                    "corrupt",
                    "PDF font map has invalid hexadecimal data.",
                ));
            }
            if digits.len() > 64 {
                return Err(fail(
                    "resource_limit",
                    "PDF font map target exceeds 16 UTF-16 code units.",
                ));
            }
            let decoded = digits
                .chunks_exact(2)
                .map(|pair| {
                    let digit = |b: u8| {
                        (b as char).to_digit(16).ok_or_else(|| {
                            fail("corrupt", "PDF font map has invalid hexadecimal data.")
                        })
                    };
                    Ok((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
                })
                .collect::<ExtractResult<Vec<_>>>()?;
            tokens.push(Token::Hex(decoded));
            index += 1;
        } else if byte == b'[' || byte == b']' {
            tokens.push(if byte == b'[' {
                Token::Open
            } else {
                Token::Close
            });
            index += 1;
        } else {
            let start = index;
            index += 1;
            while index < data.len()
                && !data[index].is_ascii_whitespace()
                && !b"<>[]%".contains(&data[index])
            {
                index += 1;
            }
            tokens.push(Token::Word(
                String::from_utf8_lossy(&data[start..index]).into_owned(),
            ));
        }
        if tokens.len() > 200_000 {
            return Err(fail("resource_limit", "PDF font map has too many tokens."));
        }
    }
    let source = |token: Option<&Token>| -> ExtractResult<u64> {
        match token {
            Some(Token::Hex(bytes)) if !bytes.is_empty() && bytes.len() <= 4 => {
                Ok(bytes.iter().fold(0u64, |v, b| v * 256 + u64::from(*b)))
            }
            _ => Err(fail("corrupt", "PDF font map has invalid source codes.")),
        }
    };
    let mut index = 0;
    let mut total = 0;
    while index < tokens.len() {
        let range = matches!(&tokens[index], Token::Word(word) if word == "beginbfrange");
        let chars = matches!(&tokens[index], Token::Word(word) if word == "beginbfchar");
        if !range && !chars {
            index += 1;
            continue;
        }
        index += 1;
        while index < tokens.len()
            && !matches!(&tokens[index], Token::Word(word) if word.starts_with("endbf"))
        {
            let start = source(tokens.get(index))?;
            index += 1;
            let count = if range {
                let end = source(tokens.get(index))?;
                index += 1;
                end.checked_sub(start)
                    .and_then(|n| n.checked_add(1))
                    .ok_or_else(|| fail("corrupt", "PDF font map has reversed ranges."))?
            } else {
                1
            };
            total += count;
            if total > 65_536 {
                return Err(fail(
                    "resource_limit",
                    "PDF font map range exceeds 65536 mapped characters.",
                ));
            }
            match tokens.get(index) {
                Some(Token::Hex(_)) => index += 1,
                Some(Token::Open) if range => {
                    index += 1;
                    while matches!(tokens.get(index), Some(Token::Hex(_))) {
                        index += 1;
                    }
                    if !matches!(tokens.get(index), Some(Token::Close)) {
                        return Err(fail("corrupt", "PDF font map target array is invalid."));
                    }
                    index += 1;
                }
                _ => return Err(fail("corrupt", "PDF font map target is invalid.")),
            }
        }
    }
    Ok(total)
}

#[derive(Default)]
struct PdfBudget<'a> {
    decoded_bytes: usize,
    xobject_calls: usize,
    operations: usize,
    active_forms: HashSet<usize>,
    encodings: HashMap<usize, lopdf::Encoding<'a>>,
    unsupported_fonts: HashSet<usize>,
}

impl PdfBudget<'_> {
    fn stream_limit(&self) -> ExtractResult<usize> {
        let remaining = PDF_DECODED_BYTES.saturating_sub(self.decoded_bytes);
        if remaining == 0 {
            return Err(fail(
                "resource_limit",
                "PDF page/Form streams exceed 32 MiB of cumulative decompression.",
            ));
        }
        Ok(remaining.min(PDF_STREAM_BYTES))
    }
}

#[derive(Default)]
struct PdfPageText {
    text: String,
    needs_ocr: bool,
    incomplete: bool,
}

fn pdf_object<'a>(
    document: &'a lopdf::Document,
    object: &'a lopdf::Object,
) -> ExtractResult<&'a lopdf::Object> {
    match object {
        lopdf::Object::Reference(id) => document.get_object(*id).map_err(pdf_failure),
        _ => Ok(object),
    }
}

fn pdf_resource<'a>(
    document: &'a lopdf::Document,
    scopes: &[&'a lopdf::Dictionary],
    category: &[u8],
    name: &[u8],
) -> ExtractResult<Option<&'a lopdf::Object>> {
    for resources in scopes {
        if let Ok(category) = resources.get(category) {
            let category = pdf_object(document, category)?
                .as_dict()
                .map_err(pdf_failure)?;
            if let Ok(resource) = category.get(name) {
                return Ok(Some(resource));
            }
        }
    }
    Ok(None)
}

fn pdf_page_resources(
    document: &lopdf::Document,
    page: lopdf::ObjectId,
) -> ExtractResult<Vec<&lopdf::Dictionary>> {
    let mut resources = Vec::new();
    let mut current = page;
    let mut visited = HashSet::new();
    loop {
        if visited.len() >= 128 || !visited.insert(current) {
            return Err(fail(
                "resource_limit",
                "PDF page resource inheritance exceeds its depth/cycle limit.",
            ));
        }
        let dictionary = document.get_dictionary(current).map_err(pdf_failure)?;
        if let Ok(object) = dictionary.get(b"Resources") {
            resources.push(
                pdf_object(document, object)?
                    .as_dict()
                    .map_err(pdf_failure)?,
            );
        }
        match dictionary.get(b"Parent") {
            Ok(parent) => current = parent.as_reference().map_err(pdf_failure)?,
            Err(_) => break,
        }
    }
    Ok(resources)
}

fn pdf_font_encoding<'a>(
    document: &'a lopdf::Document,
    font: &'a lopdf::Dictionary,
) -> ExtractResult<lopdf::Encoding<'a>> {
    if font.get(b"ToUnicode").is_ok() {
        // ToUnicode describes extracted text even when a visual glyph encoding
        // also exists. The owned CMap can be cached after this temporary clone.
        let mut preferred = font.clone();
        preferred.remove(b"Encoding");
        match preferred
            .get_font_encoding_with_limit(document, PDF_STREAM_BYTES)
            .map_err(pdf_failure)?
        {
            lopdf::Encoding::UnicodeMapEncoding(map) => {
                Ok(lopdf::Encoding::UnicodeMapEncoding(map))
            }
            _ => Err(fail(
                "partial",
                "PDF font ToUnicode text mapping is unsupported.",
            )),
        }
    } else {
        if let Ok(encoding) = font.get(b"Encoding") {
            if matches!(
                pdf_object(document, encoding)?.as_name(),
                Ok(b"Identity-H" | b"Identity-V")
            ) {
                return Err(fail(
                    "partial",
                    "PDF CID font has no supported Unicode text mapping.",
                ));
            }
        }
        font.get_font_encoding_with_limit(document, PDF_STREAM_BYTES)
            .map_err(pdf_failure)
    }
}

fn bounded_pdf_page<'a>(
    document: &'a lopdf::Document,
    page: lopdf::ObjectId,
    budget: &mut PdfBudget<'a>,
) -> ExtractResult<PdfPageText> {
    let resources = pdf_page_resources(document, page)?;
    let bytes = document
        .get_page_content_with_limit(page, budget.stream_limit()?)
        .map_err(pdf_failure)?;
    budget.decoded_bytes += bytes.len();
    let mut extracted = PdfPageText::default();
    pdf_text_stream(
        document,
        &bytes,
        &resources,
        None,
        0,
        budget,
        &mut extracted,
    )?;
    Ok(extracted)
}

fn pdf_text_stream<'a>(
    document: &'a lopdf::Document,
    bytes: &[u8],
    resources: &[&'a lopdf::Dictionary],
    mut font: Option<&'a lopdf::Dictionary>,
    depth: usize,
    budget: &mut PdfBudget<'a>,
    extracted: &mut PdfPageText,
) -> ExtractResult<()> {
    let mut graphics = Vec::new();
    for operation in lopdf::content::Content::decode_iter(bytes) {
        let operation = operation.map_err(pdf_failure)?;
        budget.operations += 1;
        if budget.operations > PDF_OPERATIONS {
            return Err(fail(
                "resource_limit",
                "PDF page/Form extraction exceeds 200000 operations.",
            ));
        }
        match operation.operator.as_str() {
            "q" => {
                if graphics.len() >= 128 {
                    return Err(fail(
                        "resource_limit",
                        "PDF graphics state exceeds 128 nested saves.",
                    ));
                }
                graphics.push(font);
            }
            "Q" => {
                if let Some(saved) = graphics.pop() {
                    font = saved;
                } else {
                    extracted.incomplete = true;
                }
            }
            "Tf" => {
                font = match operation.operands.first().and_then(|o| o.as_name().ok()) {
                    Some(name) => pdf_resource(document, resources, b"Font", name)?
                        .map(|object| {
                            pdf_object(document, object)
                                .and_then(|o| o.as_dict().map_err(pdf_failure))
                        })
                        .transpose()?,
                    None => None,
                };
                if font.is_none() {
                    extracted.incomplete = true;
                }
            }
            "Tj" | "TJ" | "'" | "\"" => {
                if matches!(operation.operator.as_str(), "'" | "\"")
                    && !extracted.text.ends_with('\n')
                {
                    extracted.text.push('\n');
                }
                let operands = if operation.operator == "\"" {
                    operation.operands.get(2..).unwrap_or_default()
                } else {
                    &operation.operands
                };
                if let Some(font) = font {
                    let key = font as *const lopdf::Dictionary as usize;
                    if budget.unsupported_fonts.contains(&key) {
                        extracted.incomplete = true;
                        continue;
                    }
                    if !budget.encodings.contains_key(&key) {
                        if budget.encodings.len() + budget.unsupported_fonts.len() >= 256 {
                            return Err(fail(
                                "resource_limit",
                                "PDF extraction exceeds 256 distinct font encodings.",
                            ));
                        }
                        match pdf_font_encoding(document, font) {
                            Ok(encoding) => {
                                budget.encodings.insert(key, encoding);
                            }
                            Err(error) if error.status == "resource_limit" => return Err(error),
                            Err(_) => {
                                budget.unsupported_fonts.insert(key);
                                extracted.incomplete = true;
                                continue;
                            }
                        }
                    }
                    let encoding = &budget.encodings[&key];
                    let mut stack = operands.iter().rev().collect::<Vec<_>>();
                    while let Some(operand) = stack.pop() {
                        match operand {
                            lopdf::Object::String(bytes, _) => {
                                let decoded = match encoding.bytes_to_string(bytes) {
                                    Ok(decoded) => decoded,
                                    Err(_) => {
                                        extracted.incomplete = true;
                                        continue;
                                    }
                                };
                                if extracted.text.len().saturating_add(decoded.len()) > TEXT_BYTES {
                                    return Err(fail(
                                        "resource_limit",
                                        "PDF page text exceeds 16 MiB.",
                                    ));
                                }
                                extracted.text.push_str(&decoded);
                            }
                            lopdf::Object::Array(items) => stack.extend(items.iter().rev()),
                            lopdf::Object::Integer(value) if *value < -100 => {
                                extracted.text.push(' ')
                            }
                            lopdf::Object::Real(value) if *value < -100.0 => {
                                extracted.text.push(' ')
                            }
                            _ => {}
                        }
                    }
                } else {
                    extracted.incomplete = true;
                }
            }
            "Do" => {
                budget.xobject_calls += 1;
                if budget.xobject_calls > PDF_XOBJECT_CALLS {
                    return Err(fail(
                        "resource_limit",
                        "PDF extraction exceeds 256 XObject invocations.",
                    ));
                }
                let object = match operation.operands.first().and_then(|o| o.as_name().ok()) {
                    Some(name) => pdf_resource(document, resources, b"XObject", name)?,
                    None => None,
                };
                let Some(object) = object else {
                    extracted.incomplete = true;
                    continue;
                };
                let object = pdf_object(document, object)?;
                let Ok(stream) = object.as_stream() else {
                    extracted.incomplete = true;
                    continue;
                };
                match stream.dict.get(b"Subtype").and_then(lopdf::Object::as_name) {
                    Ok(b"Image") => extracted.needs_ocr = true,
                    Ok(b"Form") => {
                        if depth >= PDF_FORM_DEPTH {
                            return Err(fail(
                                "resource_limit",
                                "PDF Form XObject recursion exceeds 16 levels.",
                            ));
                        }
                        // Detect active-path cycles, not repeated uses: reusing a
                        // Form paints its text again and must remain searchable.
                        let identity = stream as *const lopdf::Stream as usize;
                        if !budget.active_forms.insert(identity) {
                            return Err(fail("resource_limit", "PDF Form XObject cycle detected."));
                        }
                        let mut scopes = resources.to_vec();
                        if let Ok(local) = stream.dict.get(b"Resources") {
                            scopes.insert(
                                0,
                                pdf_object(document, local)?
                                    .as_dict()
                                    .map_err(pdf_failure)?,
                            );
                        }
                        let data = stream
                            .get_plain_content_with_limit(budget.stream_limit()?)
                            .map_err(pdf_failure)?;
                        budget.decoded_bytes += data.len();
                        // A Form implicitly saves/restores graphics state; the
                        // caller's resolved font survives local resource names.
                        pdf_text_stream(
                            document,
                            &data,
                            &scopes,
                            font,
                            depth + 1,
                            budget,
                            extracted,
                        )?;
                        budget.active_forms.remove(&identity);
                    }
                    _ => extracted.incomplete = true,
                }
            }
            "BI" => extracted.needs_ocr = true,
            "ET" | "T*" if !extracted.text.ends_with('\n') => extracted.text.push('\n'),
            _ => {}
        }
        if extracted.text.len() > TEXT_BYTES {
            return Err(fail("resource_limit", "PDF page text exceeds 16 MiB."));
        }
    }
    if !graphics.is_empty() {
        extracted.incomplete = true;
    }
    Ok(())
}

enum XmlEvent {
    Start(String, HashMap<String, String>),
    End(String),
    Text(String),
}

fn local_name(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).into()
}

fn walk_xml(
    data: &[u8],
    mut visit: impl FnMut(XmlEvent) -> ExtractResult<()>,
) -> ExtractResult<()> {
    if data.len() > XML_BYTES {
        return Err(fail("resource_limit", "Office XML part exceeds 8 MiB."));
    }
    let mut reader = Reader::from_reader(data);
    let mut depth = 0usize;
    let mut count = 0;
    let mut roots = 0;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| fail("corrupt", "Office XML is malformed."))?;
        count += 1;
        if count > 500_000 {
            return Err(fail("resource_limit", "Office XML has too many nodes."));
        }
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                if depth > 128 || roots > 1 {
                    return Err(fail(
                        "resource_limit",
                        "Office XML nesting/root count exceeds its limit.",
                    ));
                }
                let name = local_name(start.name().as_ref());
                let mut attributes = HashMap::new();
                for attribute in start.attributes() {
                    let attribute = attribute
                        .map_err(|_| fail("corrupt", "Office XML attribute is malformed."))?;
                    let key = attribute.key.as_ref().to_string();
                    let value = attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| {
                            fail(
                                "corrupt",
                                "Office XML attribute contains invalid entities or encoding.",
                            )
                        })?;
                    if attributes.insert(key, value.into_owned()).is_some() {
                        return Err(fail("corrupt", "Office XML has ambiguous attributes."));
                    }
                }
                visit(XmlEvent::Start(name.clone(), attributes))?;
                if empty {
                    visit(XmlEvent::End(name))?;
                    depth -= 1;
                }
            }
            Event::End(end) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| fail("corrupt", "Office XML is unbalanced."))?;
                visit(XmlEvent::End(local_name(end.name().as_ref())))?;
            }
            Event::Text(text) => visit(XmlEvent::Text(
                text.xml_content(quick_xml::XmlVersion::Implicit1_0)
                    .into_owned(),
            ))?,
            Event::CData(text) => visit(XmlEvent::Text(
                text.xml_content(quick_xml::XmlVersion::Implicit1_0)
                    .into_owned(),
            ))?,
            Event::GeneralRef(reference) => {
                let name = reference.into_inner();
                let encoded = format!("&{};", name);
                let value = quick_xml::escape::unescape(&encoded).map_err(|_| {
                    fail(
                        "corrupt",
                        "Office XML external/custom entities are unsupported.",
                    )
                })?;
                visit(XmlEvent::Text(value.into_owned()))?;
            }
            Event::DocType(_) => {
                return Err(fail(
                    "corrupt",
                    "Office XML DTDs and external entities are forbidden.",
                ))
            }
            Event::Decl(declaration) => {
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding.map_err(|_| {
                        fail("corrupt", "Office XML encoding declaration is invalid.")
                    })?;
                    if !encoding.eq_ignore_ascii_case("UTF-8") {
                        return Err(fail("invalid_encoding", "Office XML parts must use UTF-8."));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || roots != 1 {
        return Err(fail("corrupt", "Office XML has no complete root element."));
    }
    Ok(())
}

struct OfficePackage<'a> {
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
    names: HashSet<String>,
}

impl<'a> OfficePackage<'a> {
    fn new(bytes: &'a [u8]) -> ExtractResult<Self> {
        if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]) {
            return Err(fail(
                "encrypted",
                "Password-protected OOXML containers are unsupported.",
            ));
        }
        // Reject large central-directory counts before the ZIP reader allocates
        // metadata. ZIP64 is unnecessary inside the small input/entry budgets.
        let end = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .ok_or_else(|| fail("corrupt", "Office ZIP end record is missing."))?;
        let footer = bytes
            .get(end..end + 22)
            .ok_or_else(|| fail("corrupt", "Office ZIP end record is truncated."))?;
        if u16::from_le_bytes([footer[10], footer[11]]) > 2048 {
            return Err(fail(
                "resource_limit",
                "Office ZIP central directory exceeds 2048 entries.",
            ));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| fail("corrupt", "Office ZIP container is invalid."))?;
        if archive.len() > 2048 {
            return Err(fail(
                "resource_limit",
                "Office ZIP container exceeds 2048 entries.",
            ));
        }
        let mut names = HashSet::new();
        let mut expanded = 0u64;
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|error| {
                if error.to_string().to_ascii_lowercase().contains("password") {
                    fail("encrypted", "Encrypted Office ZIP entries are unsupported.")
                } else {
                    fail("corrupt", "Office ZIP entry is invalid.")
                }
            })?;
            if entry.encrypted() {
                return Err(fail(
                    "encrypted",
                    "Encrypted Office ZIP entries are unsupported.",
                ));
            }
            let name = entry
                .name()
                .map_err(|_| fail("corrupt", "Office ZIP entry name is invalid."))?
                .into_owned();
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
                || name.len() > 512
                || name.contains('\0')
                || name.contains('\\')
                || name.starts_with('/')
                || name
                    .split('/')
                    .any(|part| part == ".." || part == "." || part.contains(':'))
                || !names.insert(name)
            {
                return Err(fail(
                    "corrupt",
                    "Office ZIP has duplicate or unsafe entry names.",
                ));
            }
            expanded = expanded
                .checked_add(entry.size())
                .ok_or_else(|| fail("resource_limit", "Office ZIP size overflows."))?;
            if expanded > EXPANDED_BYTES
                || entry.size() > 16 * 1024 * 1024
                || (entry.size() > 1024 * 1024
                    && entry.size() > entry.compressed_size().max(1).saturating_mul(200))
            {
                return Err(fail("resource_limit", "Office ZIP exceeds expansion limits (32 MiB total, 16 MiB per entry, 200:1 for entries over 1 MiB)."));
            }
        }
        Ok(Self { archive, names })
    }

    fn xml(&mut self, name: &str) -> ExtractResult<Vec<u8>> {
        let entry = self.archive.by_name(name).map_err(|_| {
            fail(
                "corrupt",
                format!("Required Office part is missing: {name}"),
            )
        })?;
        if entry.size() > XML_BYTES as u64 {
            return Err(fail("resource_limit", "Office XML part exceeds 8 MiB."));
        }
        let mut bytes = Vec::new();
        entry
            .take(XML_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                fail(
                    "corrupt",
                    "Office XML could not be decompressed or failed CRC validation.",
                )
            })?;
        if bytes.len() > XML_BYTES {
            return Err(fail("resource_limit", "Office XML part exceeds 8 MiB."));
        }
        Ok(bytes)
    }
}

fn office_content(
    bytes: &[u8],
    format: &str,
    options: &ReadOptions,
) -> ExtractResult<Vec<ContentUnit>> {
    let mut package = OfficePackage::new(bytes)?;
    // OOXML relationships only choose bounded members of this in-memory archive.
    // They are never opened as filesystem paths or fetched as external URLs.
    match format {
        "docx" => {
            let data = package.xml("word/document.xml")?;
            let paragraphs = paragraphs(&data)?;
            let mut units = Vec::new();
            let mut total = 0;
            for (index, text) in paragraphs.into_iter().enumerate() {
                push_unit(
                    &mut units,
                    &mut total,
                    format!("paragraph:{}", index + 1),
                    text,
                )?;
            }
            Ok(units)
        }
        "pptx" => {
            let presentation = package.xml("ppt/presentation.xml")?;
            let relationships = package.xml("ppt/_rels/presentation.xml.rels")?;
            let targets = relationships_map(&relationships, "ppt")?;
            let mut slides = Vec::new();
            walk_xml(&presentation, |event| {
                if let XmlEvent::Start(name, attrs) = event {
                    if name == "sldId" {
                        slides.push(
                            relationship_id(&attrs)
                                .ok_or_else(|| {
                                    fail("corrupt", "Slide relationship id is missing.")
                                })?
                                .to_string(),
                        );
                    }
                }
                Ok(())
            })?;
            if slides.len() > 2000 {
                return Err(fail("resource_limit", "Presentation exceeds 2000 slides."));
            }
            let mut units = Vec::new();
            let mut total = 0;
            for (index, id) in slides.into_iter().enumerate() {
                let target = targets
                    .get(&id)
                    .ok_or_else(|| fail("corrupt", "Slide relationship is missing or external."))?;
                let text = paragraphs(&package.xml(target)?)?.join("\n");
                push_unit(&mut units, &mut total, format!("slide:{}", index + 1), text)?;
            }
            Ok(units)
        }
        "xlsx" => workbook_content(&mut package, options),
        _ => unreachable!(),
    }
}

fn paragraphs(data: &[u8]) -> ExtractResult<Vec<String>> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_paragraph = 0usize;
    let mut in_text = 0usize;
    let mut total = 0usize;
    walk_xml(data, |event| {
        match event {
            XmlEvent::Start(name, _) if name == "p" => {
                if in_paragraph != 0 {
                    return Err(fail("corrupt", "Nested Office paragraphs are unsupported."));
                }
                in_paragraph = 1;
                current.clear();
            }
            XmlEvent::Start(name, _) if name == "t" && in_paragraph > 0 => in_text += 1,
            XmlEvent::Start(name, _) if name == "tab" && in_paragraph > 0 => current.push('\t'),
            XmlEvent::Start(name, _)
                if matches!(name.as_str(), "br" | "cr") && in_paragraph > 0 =>
            {
                current.push('\n')
            }
            XmlEvent::Text(text) if in_text > 0 => current.push_str(&text),
            XmlEvent::End(name) if name == "t" && in_text > 0 => in_text -= 1,
            XmlEvent::End(name) if name == "p" => {
                total += current.len();
                if result.len() >= MAX_UNITS || total > TEXT_BYTES {
                    return Err(fail(
                        "resource_limit",
                        "Office text exceeds its extraction limit.",
                    ));
                }
                result.push(std::mem::take(&mut current));
                in_paragraph = 0;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(result)
}

fn relationships_map(data: &[u8], base: &str) -> ExtractResult<HashMap<String, String>> {
    let mut map = HashMap::new();
    walk_xml(data, |event| {
        if let XmlEvent::Start(name, attrs) = event {
            if name == "Relationship" {
                if attrs
                    .get("TargetMode")
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("external"))
                {
                    return Ok(());
                }
                let id = attrs
                    .get("Id")
                    .ok_or_else(|| fail("corrupt", "Office relationship id is missing."))?;
                let target = attrs
                    .get("Target")
                    .ok_or_else(|| fail("corrupt", "Office relationship target is missing."))?;
                if target.contains('\\')
                    || target.contains(':')
                    || target.contains('?')
                    || target.contains('#')
                {
                    return Err(fail("corrupt", "Office relationship target is unsafe."));
                }
                let mut parts = if target.starts_with('/') {
                    vec![]
                } else {
                    base.split('/').collect::<Vec<_>>()
                };
                for part in target.trim_start_matches('/').split('/') {
                    match part {
                        ".." => {
                            if parts.pop().is_none() {
                                return Err(fail(
                                    "corrupt",
                                    "Office relationship escapes the package.",
                                ));
                            }
                        }
                        "." | "" => {}
                        _ => parts.push(part),
                    }
                }
                if map.insert(id.clone(), parts.join("/")).is_some() {
                    return Err(fail("corrupt", "Office relationship ids are duplicated."));
                }
            }
        }
        Ok(())
    })?;
    Ok(map)
}

fn relationship_id(attrs: &HashMap<String, String>) -> Option<&str> {
    attrs
        .iter()
        .find(|(key, _)| key.ends_with(":id"))
        .map(|(_, value)| value.as_str())
}

fn workbook_content(
    package: &mut OfficePackage<'_>,
    options: &ReadOptions,
) -> ExtractResult<Vec<ContentUnit>> {
    let data = package.xml("xl/workbook.xml")?;
    let targets = relationships_map(&package.xml("xl/_rels/workbook.xml.rels")?, "xl")?;
    let mut sheets = Vec::new();
    walk_xml(&data, |event| {
        if let XmlEvent::Start(name, attrs) = event {
            if name == "sheet" {
                let name = attrs
                    .get("name")
                    .ok_or_else(|| fail("corrupt", "Workbook sheet name is missing."))?;
                if name.len() > 256 {
                    return Err(fail("resource_limit", "Workbook sheet name is too long."));
                }
                let id = relationship_id(&attrs)
                    .ok_or_else(|| fail("corrupt", "Workbook sheet relationship is missing."))?;
                sheets.push((name.clone(), id.to_string()));
            }
        }
        Ok(())
    })?;
    if sheets.len() > 256 {
        return Err(fail("resource_limit", "Workbook exceeds 256 sheets."));
    }
    if options
        .sheet
        .as_ref()
        .is_some_and(|selected| !sheets.iter().any(|(name, _)| name == selected))
    {
        return Err(fail(
            "invalid_selection",
            "Requested worksheet does not exist.",
        ));
    }
    let range = options.range.as_deref().map(cell_range).transpose()?;
    if range.is_some() && options.sheet.is_none() {
        return Err(fail(
            "invalid_selection",
            "An XLSX range requires an explicit sheet name.",
        ));
    }
    let strings = if package.names.contains("xl/sharedStrings.xml") {
        shared_strings(&package.xml("xl/sharedStrings.xml")?)?
    } else {
        vec![]
    };
    let mut units = Vec::new();
    let mut total = 0;
    for (name, id) in sheets {
        if options
            .sheet
            .as_ref()
            .is_some_and(|selected| selected != &name)
        {
            continue;
        }
        let path = targets
            .get(&id)
            .ok_or_else(|| fail("corrupt", "Worksheet relationship is missing or external."))?;
        let data = package.xml(path)?;
        let cells = sheet_cells(&data, &strings, range)?;
        for (address, text) in cells {
            push_unit(
                &mut units,
                &mut total,
                format!("sheet:{name}!{address}"),
                text,
            )?;
        }
    }
    Ok(units)
}

fn shared_strings(data: &[u8]) -> ExtractResult<Vec<String>> {
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut active = false;
    let mut text_active = false;
    let mut total = 0usize;
    walk_xml(data, |event| {
        match event {
            XmlEvent::Start(name, _) if name == "si" => {
                active = true;
                current.clear();
            }
            XmlEvent::Start(name, _) if name == "t" && active => text_active = true,
            XmlEvent::Text(text) if text_active => current.push_str(&text),
            XmlEvent::End(name) if name == "t" => text_active = false,
            XmlEvent::End(name) if name == "si" => {
                total += current.len();
                if strings.len() >= MAX_UNITS || total > TEXT_BYTES {
                    return Err(fail(
                        "resource_limit",
                        "Workbook shared strings exceed their limit.",
                    ));
                }
                strings.push(std::mem::take(&mut current));
                active = false;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(strings)
}

type CellRange = ((u32, u32), (u32, u32));

fn cell_position(address: &str) -> ExtractResult<(u32, u32)> {
    let bytes = address.as_bytes();
    let mut index = 0;
    let mut column = 0u32;
    if bytes.get(index) == Some(&b'$') {
        index += 1;
    }
    let start = index;
    while bytes.get(index).is_some_and(|b| b.is_ascii_alphabetic()) {
        column = column
            .checked_mul(26)
            .and_then(|v| v.checked_add(u32::from(bytes[index].to_ascii_uppercase() - b'A' + 1)))
            .ok_or_else(|| fail("invalid_selection", "XLSX cell column is invalid."))?;
        index += 1;
    }
    if index == start {
        return Err(fail(
            "invalid_selection",
            "XLSX ranges must use A1 cell addresses.",
        ));
    }
    if bytes.get(index) == Some(&b'$') {
        index += 1;
    }
    let row_text = std::str::from_utf8(&bytes[index..])
        .map_err(|_| fail("invalid_selection", "XLSX row is invalid."))?;
    if row_text.is_empty() || !row_text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(fail("invalid_selection", "XLSX row is invalid."));
    }
    let row = row_text
        .parse::<u32>()
        .map_err(|_| fail("invalid_selection", "XLSX row is invalid."))?;
    if column == 0 || column > 16_384 || row == 0 || row > 1_048_576 {
        return Err(fail(
            "invalid_selection",
            "XLSX cell is outside worksheet bounds.",
        ));
    }
    Ok((column, row))
}

fn cell_range(range: &str) -> ExtractResult<CellRange> {
    if range.len() > 64 {
        return Err(fail("invalid_selection", "XLSX range is too long."));
    }
    let mut parts = range.split(':');
    let start = cell_position(parts.next().unwrap_or_default())?;
    let end = parts
        .next()
        .map(cell_position)
        .transpose()?
        .unwrap_or(start);
    if parts.next().is_some() || end.0 < start.0 || end.1 < start.1 {
        return Err(fail(
            "invalid_selection",
            "XLSX range must be a forward A1:B2 rectangle.",
        ));
    }
    Ok((start, end))
}

fn sheet_cells(
    data: &[u8],
    shared: &[String],
    range: Option<CellRange>,
) -> ExtractResult<Vec<(String, String)>> {
    let mut cells = Vec::new();
    let mut address = String::new();
    let mut kind = String::new();
    let mut value = String::new();
    let mut formula = String::new();
    let mut active = false;
    let mut collecting = false;
    let mut in_formula = false;
    let mut total = 0usize;
    let mut visited = 0usize;
    let mut positions = HashSet::new();
    walk_xml(data, |event| {
        match event {
            XmlEvent::Start(name, attrs) if name == "c" => {
                visited += 1;
                if active || visited > MAX_UNITS {
                    return Err(fail(
                        "resource_limit",
                        "Worksheet has too many or nested cells.",
                    ));
                }
                address = attrs
                    .get("r")
                    .ok_or_else(|| fail("corrupt", "Worksheet cell address is missing."))?
                    .clone();
                let position = cell_position(&address)
                    .map_err(|_| fail("corrupt", "Worksheet cell address is invalid."))?;
                if !positions.insert(position) {
                    return Err(fail("corrupt", "Worksheet cell address is duplicated."));
                }
                kind = attrs.get("t").cloned().unwrap_or_default();
                active = true;
                value.clear();
                formula.clear();
            }
            XmlEvent::Start(name, _) if matches!(name.as_str(), "v" | "t") && active => {
                collecting = true
            }
            XmlEvent::Start(name, _) if name == "f" && active => in_formula = true,
            XmlEvent::Text(text) if collecting => value.push_str(&text),
            XmlEvent::Text(text) if in_formula => formula.push_str(&text),
            XmlEvent::End(name) if matches!(name.as_str(), "v" | "t") => collecting = false,
            XmlEvent::End(name) if name == "f" => in_formula = false,
            XmlEvent::End(name) if name == "c" => {
                active = false;
                let position = cell_position(&address)?;
                if range.is_some_and(|(start, end)| {
                    position.0 < start.0
                        || position.0 > end.0
                        || position.1 < start.1
                        || position.1 > end.1
                }) {
                    return Ok(());
                }
                let text = if kind == "s" {
                    let index = value.parse::<usize>().map_err(|_| {
                        fail("corrupt", "Worksheet shared-string index is invalid.")
                    })?;
                    shared
                        .get(index)
                        .ok_or_else(|| {
                            fail("corrupt", "Worksheet shared-string index is missing.")
                        })?
                        .clone()
                } else if kind == "b" {
                    match value.as_str() {
                        "1" => "true".into(),
                        "0" => "false".into(),
                        _ => return Err(fail("corrupt", "Worksheet boolean is invalid.")),
                    }
                } else {
                    value.clone()
                };
                // Cached values are displayed as stored; formulas are never evaluated.
                let text = if !formula.is_empty() {
                    format!("{text}\n[formula: ={formula}; cached value, not evaluated]")
                } else {
                    text
                };
                total += text.len();
                if total > TEXT_BYTES {
                    return Err(fail("resource_limit", "Worksheet text exceeds 16 MiB."));
                }
                cells.push((address.clone(), text));
            }
            _ => {}
        }
        Ok(())
    })?;
    // XML permits sparse rows and cells. Sorting real addresses gives stable,
    // row-major units without allocating the potentially enormous empty range.
    cells.sort_by_key(|(address, _)| {
        let (column, row) = cell_position(address).expect("validated address");
        (row, column)
    });
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const DOCX: &[u8] = include_bytes!("../tests/fixtures/sample.docx");
    const PPTX: &[u8] = include_bytes!("../tests/fixtures/sample.pptx");
    const XLSX: &[u8] = include_bytes!("../tests/fixtures/sample.xlsx");
    const PDF: &[u8] = include_bytes!("../tests/fixtures/sample.pdf");
    const PNG: &[u8] = include_bytes!("../tests/fixtures/sample.png");

    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in parts {
            writer
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(data.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn options() -> ReadOptions {
        ReadOptions::default()
    }

    #[test]
    fn real_text_fixture_preserves_line_breaks_and_strict_encoding() {
        let bytes = include_bytes!("../tests/fixtures/sample.txt");
        let result = read_bytes(bytes, "notes.txt", &options()).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.units.len(), 3);
        assert_eq!(result.units[0].location, "line:1");
        assert_eq!(
            result
                .units
                .iter()
                .map(|unit| unit.text.as_str())
                .collect::<String>(),
            std::str::from_utf8(bytes).unwrap()
        );
        assert_eq!(
            read_bytes(&[0x66, 0x80], "bad.txt", &options())
                .unwrap()
                .status,
            "invalid_encoding"
        );
        assert_eq!(
            read_bytes(b"a\0b", "binary.txt", &options())
                .unwrap()
                .status,
            "invalid_encoding"
        );
    }

    #[test]
    fn utf16_bom_requires_valid_complete_surrogates() {
        let bytes = [0xff, 0xfe, 0x2d, 0x4e, 0x87, 0x65];
        assert_eq!(
            read_bytes(&bytes, "notes.txt", &options()).unwrap().units[0].text,
            "中文"
        );
        assert_eq!(
            read_bytes(&[0xff, 0xfe, 0x00, 0xd8], "bad.txt", &options())
                .unwrap()
                .status,
            "invalid_encoding"
        );
        assert_eq!(
            read_bytes(&[0xfe, 0xff, 0x00], "bad.txt", &options())
                .unwrap()
                .status,
            "invalid_encoding"
        );
    }

    #[test]
    fn long_multibyte_line_continuation_has_no_missing_tails() {
        let text = format!("{}\nsecond\n", "中文ABC".repeat(500));
        let mut opts = ReadOptions {
            max_bytes: 128,
            limit: 1,
            ..options()
        };
        let mut collected = String::new();
        let mut calls = 0;
        loop {
            let result = read_bytes(text.as_bytes(), "long.txt", &opts).unwrap();
            assert!(
                result
                    .units
                    .iter()
                    .map(|unit| unit.text.len())
                    .sum::<usize>()
                    <= 128
            );
            collected.extend(result.units.into_iter().map(|unit| unit.text));
            calls += 1;
            if let Some(start) = result.next_start {
                opts.start = start;
                opts.offset = result.next_offset;
            } else {
                break;
            }
            assert!(calls < 100);
        }
        assert_eq!(collected, text);
        assert!(calls > 2);
        opts.start = 0;
        opts.offset = 1;
        assert_eq!(
            read_bytes(text.as_bytes(), "long.txt", &opts)
                .unwrap()
                .status,
            "invalid_position"
        );
    }

    #[test]
    fn real_docx_paragraphs_decode_entities_tabs_and_breaks() {
        let result = read_bytes(DOCX, "sample.docx", &options()).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.units.len(), 2);
        assert_eq!(result.units[0].text, "第一段 & bounded text\tnext");
        assert_eq!(result.units[1].text, "Second paragraph\nnew line");
        assert_eq!(result.units[1].location, "paragraph:2");
    }

    #[test]
    fn docx_large_paragraph_is_resumable() {
        let text = "long 中文 paragraph ".repeat(100);
        let xml = format!("<document><body><p><r><t>{text}</t></r></p></body></document>");
        let bytes = package(&[("word/document.xml", &xml)]);
        let mut opts = ReadOptions {
            max_bytes: 128,
            ..options()
        };
        let mut collected = String::new();
        loop {
            let result = read_bytes(&bytes, "long.docx", &opts).unwrap();
            collected.extend(result.units.into_iter().map(|u| u.text));
            match result.next_start {
                Some(start) => {
                    opts.start = start;
                    opts.offset = result.next_offset;
                }
                None => break,
            }
        }
        assert_eq!(collected, text);
    }

    #[test]
    fn real_pptx_uses_presentation_order_and_ignores_external_targets() {
        let result = read_bytes(PPTX, "sample.pptx", &options()).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.units.len(), 2);
        assert_eq!(result.units[0].location, "slide:1");
        assert_eq!(result.units[0].text, "First displayed slide");
        assert_eq!(result.units[1].text, "Second displayed slide");
    }

    #[test]
    fn real_xlsx_selects_named_sheet_and_range_without_evaluating_formulas() {
        let opts = ReadOptions {
            sheet: Some("数据".into()),
            range: Some("$B$1:C2".into()),
            ..options()
        };
        let result = read_bytes(XLSX, "sample.xlsx", &opts).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.units.len(), 4);
        assert_eq!(result.units[0].location, "sheet:数据!B1");
        assert_eq!(result.units[0].text, "42");
        assert!(result.units[1].text.starts_with("84\n[formula: =B1*2;"));
        assert_eq!(result.units[2].text, "true");
        assert_eq!(result.units[3].text, "Rich text");
        let all = read_bytes(XLSX, "sample.xlsx", &options()).unwrap();
        assert_eq!(all.units.len(), 7);
        assert_eq!(all.units[0].text, "Shared 中文");
        assert_eq!(all.units[6].text, "Other sheet");
    }

    #[test]
    fn xlsx_rejects_invalid_ranges_missing_sheets_and_bad_shared_strings() {
        for range in ["B2:A1", "A0:B1", "XFE1", "A1:B2:C3"] {
            let opts = ReadOptions {
                sheet: Some("数据".into()),
                range: Some(range.into()),
                ..options()
            };
            assert_eq!(
                read_bytes(XLSX, "sample.xlsx", &opts).unwrap().status,
                "invalid_selection"
            );
        }
        let opts = ReadOptions {
            sheet: Some("missing".into()),
            ..options()
        };
        assert_eq!(
            read_bytes(XLSX, "sample.xlsx", &opts).unwrap().status,
            "invalid_selection"
        );
        let opts = ReadOptions {
            range: Some("A1".into()),
            ..options()
        };
        assert_eq!(
            read_bytes(XLSX, "sample.xlsx", &opts).unwrap().status,
            "invalid_selection"
        );
    }

    #[test]
    fn real_pdf_text_and_scanned_pdf_have_distinct_status() {
        let result = read_bytes(PDF, "sample.pdf", &options()).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.units[0].location, "page:1");
        assert_eq!(result.units[0].text.trim(), "PDF page one");
        let scanned = read_bytes(
            include_bytes!("../tests/fixtures/scanned.pdf"),
            "scanned.pdf",
            &options(),
        )
        .unwrap();
        assert_eq!(scanned.status, "requires_ocr");
        assert!(scanned.units[0].text.trim().is_empty());
        assert!(scanned.message.unwrap().contains("OCR"));
    }

    fn form_object(data: &[u8], resources: Option<lopdf::Dictionary>) -> lopdf::Object {
        let mut dictionary = lopdf::dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![lopdf::Object::Integer(0),lopdf::Object::Integer(0),lopdf::Object::Integer(100),lopdf::Object::Integer(100)] };
        if let Some(resources) = resources {
            dictionary.set("Resources", resources);
        }
        lopdf::Object::Stream(lopdf::Stream::new(dictionary, data.to_vec()))
    }

    fn pdf_bytes(mut document: lopdf::Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    fn pdf_page_stream(document: &mut lopdf::Document, data: &[u8]) {
        document.objects.insert(
            (4, 0),
            lopdf::Object::Stream(lopdf::Stream::new(lopdf::Dictionary::new(), data.to_vec())),
        );
    }

    fn pdf_page_form(document: &mut lopdf::Document, form: lopdf::ObjectId) {
        document.get_object_mut((3,0)).unwrap().as_dict_mut().unwrap().set("Resources",lopdf::dictionary! { "Font" => lopdf::dictionary! { "F1" => (5,0) }, "XObject" => lopdf::dictionary! { "Form1" => form } });
    }

    #[test]
    fn pdf_direct_and_form_text_are_extracted_in_content_order() {
        let result = read_bytes(
            include_bytes!("../tests/fixtures/form.pdf"),
            "form.pdf",
            &options(),
        )
        .unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.units[0].text.trim(), "Visible heading\nFORM_NEEDLE");
        assert!(result.next_start.is_none());
    }

    #[test]
    fn pdf_nested_forms_inherit_resources_override_fonts_and_restore_caller_state() {
        let mut document =
            lopdf::Document::load_mem(include_bytes!("../tests/fixtures/form.pdf")).unwrap();
        let local_font = document.add_object(lopdf::dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => lopdf::dictionary! { "BaseEncoding" => "WinAnsiEncoding", "Differences" => vec![lopdf::Object::Integer(65),lopdf::Object::Name(b"Z".to_vec())] } });
        let inner_resources = document
            .add_object(lopdf::dictionary! { "Font" => lopdf::dictionary! { "F1" => local_font } });
        let mut inner = form_object(b"BT (INHERITED) Tj ET BT /F1 12 Tf (AB) Tj ET", None);
        if let lopdf::Object::Stream(stream) = &mut inner {
            stream.dict.set("Resources", inner_resources);
        }
        let inner = document.add_object(inner);
        document.objects.insert(
            (6, 0),
            form_object(
                b"BT /F1 12 Tf (OUTER) Tj ET /Inner Do",
                Some(lopdf::dictionary! { "XObject" => lopdf::dictionary! { "Inner" => inner } }),
            ),
        );
        pdf_page_stream(
            &mut document,
            b"BT /F1 12 Tf (Visible heading) Tj ET /Form1 Do BT (AFTER) Tj ET",
        );
        let result = read_bytes(&pdf_bytes(document), "nested.pdf", &options()).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(
            result.units[0].text.trim(),
            "Visible heading\nOUTER\nINHERITED\nZB\nAFTER"
        );
    }

    #[test]
    fn pdf_forms_inherit_page_ancestor_resources_and_repeat_visible_text() {
        let mut document =
            lopdf::Document::load_mem(include_bytes!("../tests/fixtures/form.pdf")).unwrap();
        document.objects.insert(
            (6, 0),
            form_object(b"BT /F1 12 Tf (FORM_NEEDLE) Tj ET", None),
        );
        let page = document
            .get_object_mut((3, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap();
        let resources = page.remove(b"Resources").unwrap();
        document
            .get_object_mut((2, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Resources", resources);
        pdf_page_stream(&mut document, b"/Form1 Do /Form1 Do");
        let result = read_bytes(&pdf_bytes(document), "inherited.pdf", &options()).unwrap();
        assert_eq!(result.status, "ok");
        assert_eq!(result.units[0].text.matches("FORM_NEEDLE").count(), 2);
    }

    #[test]
    fn pdf_form_active_path_cycles_are_explicit_resource_limits() {
        for indirect in [false, true] {
            let mut document =
                lopdf::Document::load_mem(include_bytes!("../tests/fixtures/form.pdf")).unwrap();
            let next = if indirect {
                document.add_object(form_object(
                    b"/Again Do",
                    Some(
                        lopdf::dictionary! { "XObject" => lopdf::dictionary! { "Again" => (6,0) } },
                    ),
                ))
            } else {
                (6, 0)
            };
            document.objects.insert(
                (6, 0),
                form_object(
                    b"/Again Do",
                    Some(
                        lopdf::dictionary! { "XObject" => lopdf::dictionary! { "Again" => next } },
                    ),
                ),
            );
            let result = read_bytes(&pdf_bytes(document), "cycle.pdf", &options()).unwrap();
            assert_eq!(result.status, "resource_limit");
            assert!(result.message.unwrap().contains("cycle"));
        }
    }

    #[test]
    fn pdf_form_recursion_depth_is_bounded_without_rejecting_the_boundary() {
        for count in [PDF_FORM_DEPTH, PDF_FORM_DEPTH + 1] {
            let mut document = lopdf::Document::load_mem(PDF).unwrap();
            let mut next =
                document.add_object(form_object(b"BT /F1 12 Tf (DEEP_NEEDLE) Tj ET", None));
            for _ in 1..count {
                next = document.add_object(form_object(
                    b"/Next Do",
                    Some(lopdf::dictionary! { "XObject" => lopdf::dictionary! { "Next" => next } }),
                ));
            }
            pdf_page_form(&mut document, next);
            pdf_page_stream(&mut document, b"/Form1 Do");
            let result = read_bytes(&pdf_bytes(document), "depth.pdf", &options()).unwrap();
            if count == PDF_FORM_DEPTH {
                assert_eq!(result.status, "ok");
                assert!(result.units[0].text.contains("DEEP_NEEDLE"));
            } else {
                assert_eq!(result.status, "resource_limit");
                assert!(result.message.unwrap().contains("16 levels"));
            }
        }
    }

    #[test]
    fn pdf_form_invocation_count_and_cumulative_decompression_are_bounded() {
        for count in [PDF_XOBJECT_CALLS, PDF_XOBJECT_CALLS + 1] {
            let mut document =
                lopdf::Document::load_mem(include_bytes!("../tests/fixtures/form.pdf")).unwrap();
            pdf_page_stream(&mut document, "/Form1 Do\n".repeat(count).as_bytes());
            let result = read_bytes(&pdf_bytes(document), "calls.pdf", &options()).unwrap();
            if count == PDF_XOBJECT_CALLS {
                assert_eq!(result.status, "ok");
                assert_eq!(result.units[0].text.matches("FORM_NEEDLE").count(), count);
            } else {
                assert_eq!(result.status, "resource_limit");
                assert!(result.message.unwrap().contains("256 XObject"));
            }
        }
        let mut document =
            lopdf::Document::load_mem(include_bytes!("../tests/fixtures/form.pdf")).unwrap();
        let mut form = form_object(&vec![b' '; PDF_STREAM_BYTES], None);
        if let lopdf::Object::Stream(stream) = &mut form {
            stream.compress().unwrap();
        }
        document.objects.insert((6, 0), form);
        pdf_page_stream(
            &mut document,
            "/Form1 Do\n"
                .repeat(PDF_DECODED_BYTES / PDF_STREAM_BYTES + 1)
                .as_bytes(),
        );
        let result = read_bytes(&pdf_bytes(document), "decoded-budget.pdf", &options()).unwrap();
        assert_eq!(result.status, "resource_limit");
    }

    #[test]
    fn pdf_mixed_image_inline_image_and_form_image_preserve_text_with_ocr_gap() {
        let result = read_bytes(
            include_bytes!("../tests/fixtures/mixed-image.pdf"),
            "mixed-image.pdf",
            &options(),
        )
        .unwrap();
        assert_eq!(result.status, "partial");
        assert_eq!(result.units[0].text.trim(), "Visible heading");
        assert!(result.message.unwrap().contains("OCR"));
        let mut document = lopdf::Document::load_mem(PDF).unwrap();
        let image_form = document.add_object(form_object(b"/Im1 Do", None));
        document.get_object_mut((3,0)).unwrap().as_dict_mut().unwrap().set("Resources",lopdf::dictionary! { "Font" => lopdf::dictionary! { "F1" => (5,0) }, "XObject" => lopdf::dictionary! { "Im1" => (6,0), "Form1" => image_form } });
        pdf_page_stream(
            &mut document,
            b"BT /F1 12 Tf (Visible heading) Tj ET /Form1 Do",
        );
        let result = read_bytes(&pdf_bytes(document), "form-image.pdf", &options()).unwrap();
        assert_eq!(result.status, "partial");
        assert_eq!(result.units[0].text.trim(), "Visible heading");
        let mut document = lopdf::Document::load_mem(PDF).unwrap();
        pdf_page_stream(&mut document,b"BT /F1 12 Tf (Visible heading) Tj ET BI /W 1 /H 1 /BPC 8 /CS /G ID X EI BT /F1 12 Tf (AFTER) Tj ET");
        let result = read_bytes(&pdf_bytes(document), "inline.pdf", &options()).unwrap();
        assert_eq!(result.status, "partial");
        assert_eq!(result.units[0].text.trim(), "Visible heading\nAFTER");
    }

    #[test]
    fn pdf_unresolved_or_unsupported_xobjects_do_not_claim_complete_extraction() {
        for subtype in ["missing", "PS"] {
            let mut document =
                lopdf::Document::load_mem(include_bytes!("../tests/fixtures/form.pdf")).unwrap();
            if subtype == "missing" {
                pdf_page_stream(
                    &mut document,
                    b"BT /F1 12 Tf (Visible heading) Tj ET /Missing Do",
                );
            } else if let lopdf::Object::Stream(stream) = document.get_object_mut((6, 0)).unwrap() {
                stream.dict.set("Subtype", subtype);
            }
            let result = read_bytes(&pdf_bytes(document), "unsupported.pdf", &options()).unwrap();
            assert_eq!(result.status, "partial");
            assert_eq!(result.units[0].text.trim(), "Visible heading");
        }
    }

    #[test]
    fn pdf_page_continuation_preserves_text_and_validates_offsets() {
        let mut document = lopdf::Document::load_mem(PDF).unwrap();
        let text = "A".repeat(500);
        let stream = format!("BT /F1 12 Tf ({text}) Tj ET");
        document.objects.insert(
            (4, 0),
            lopdf::Object::Stream(lopdf::Stream::new(
                lopdf::Dictionary::new(),
                stream.into_bytes(),
            )),
        );
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let mut opts = ReadOptions {
            max_bytes: 128,
            ..options()
        };
        let mut result_text = String::new();
        loop {
            let result = read_bytes(&bytes, "long.pdf", &opts).unwrap();
            assert_ne!(result.status, "corrupt");
            result_text.extend(result.units.into_iter().map(|u| u.text));
            if let Some(start) = result.next_start {
                opts.start = start;
                opts.offset = result.next_offset;
            } else {
                break;
            }
        }
        assert_eq!(result_text.trim(), text);
    }

    #[test]
    fn pdf_rejects_compressed_stream_and_unicode_range_bombs() {
        let mut document = lopdf::Document::load_mem(PDF).unwrap();
        let mut stream =
            lopdf::Stream::new(lopdf::Dictionary::new(), vec![b' '; PDF_STREAM_BYTES + 1]);
        stream.compress().unwrap();
        document
            .objects
            .insert((4, 0), lopdf::Object::Stream(stream));
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert_eq!(
            read_bytes(&bytes, "bomb.pdf", &options()).unwrap().status,
            "resource_limit"
        );
        let cmap = b"1 beginbfrange\n<00000000> <FFFFFFFF> <0041>\nendbfrange";
        assert_eq!(preflight_cmap(cmap).unwrap_err().status, "resource_limit");
        let id = document.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), cmap.to_vec()));
        document
            .get_object_mut((5, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("ToUnicode", id);
        bytes.clear();
        document.save_to(&mut bytes).unwrap();
        assert_eq!(
            read_bytes(&bytes, "font-bomb.pdf", &options())
                .unwrap()
                .status,
            "resource_limit"
        );
    }

    #[test]
    fn pdf_unicode_cmap_and_encryption_are_handled_before_text_extraction() {
        let mut document = lopdf::Document::load_mem(PDF).unwrap();
        let cmap = b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Test def\n/CMapType 2 def\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <4E2D>\n<42> <6587>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend end";
        let id = document.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), cmap.to_vec()));
        document
            .get_object_mut((5, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("ToUnicode", id);
        document.objects.insert(
            (4, 0),
            lopdf::Object::Stream(lopdf::Stream::new(
                lopdf::Dictionary::new(),
                b"BT /F1 12 Tf (AB) Tj ET".to_vec(),
            )),
        );
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let read = read_bytes(&bytes, "unicode.pdf", &options()).unwrap();
        assert_eq!(read.status, "ok");
        assert_eq!(read.units[0].text.trim(), "中文");
        document.trailer.set(
            "ID",
            vec![
                lopdf::Object::string_literal("fixture-file-id-1"),
                lopdf::Object::string_literal("fixture-file-id-1"),
            ],
        );
        let version = lopdf::EncryptionVersion::V1 {
            document: &document,
            owner_password: "owner",
            user_password: "secret",
            permissions: lopdf::Permissions::empty(),
        };
        let state = lopdf::EncryptionState::try_from(version).unwrap();
        document.encrypt(&state).unwrap();
        bytes.clear();
        document.save_to(&mut bytes).unwrap();
        assert_eq!(
            read_bytes(&bytes, "encrypted.pdf", &options())
                .unwrap()
                .status,
            "encrypted"
        );
    }

    #[test]
    fn real_image_returns_binary_thumbnail_or_exact_opt_in_original() {
        let result = read_bytes(PNG, "sample.png", &options()).unwrap();
        assert_eq!(result.status, "requires_ocr");
        let thumbnail = result.image.unwrap();
        assert_eq!(thumbnail.mime_type, "image/png");
        assert!(thumbnail.bytes.starts_with(b"\x89PNG"));
        assert!(thumbnail.bytes.len() <= options().max_bytes);
        let opts = ReadOptions {
            original: true,
            ..options()
        };
        assert_eq!(
            read_bytes(PNG, "sample.png", &opts)
                .unwrap()
                .image
                .unwrap()
                .bytes,
            PNG
        );
    }

    #[test]
    fn all_enabled_image_formats_decode_locally_and_honor_byte_dimension_limits() {
        for (format, extension) in [
            (ImageFormat::Png, "png"),
            (ImageFormat::Jpeg, "jpg"),
            (ImageFormat::Gif, "gif"),
            (ImageFormat::WebP, "webp"),
            (ImageFormat::Bmp, "bmp"),
        ] {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::new(4, 3));
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, format).unwrap();
            assert_eq!(
                read_bytes(bytes.get_ref(), &format!("sample.{extension}"), &options())
                    .unwrap()
                    .status,
                "requires_ocr"
            );
        }
        let wide = image::DynamicImage::ImageRgb8(image::RgbImage::new(9000, 1));
        let mut bytes = Cursor::new(Vec::new());
        wide.write_to(&mut bytes, ImageFormat::Png).unwrap();
        assert_eq!(
            read_bytes(bytes.get_ref(), "wide.png", &options())
                .unwrap()
                .status,
            "resource_limit"
        );
        let opts = ReadOptions {
            original: true,
            max_bytes: 128,
            ..options()
        };
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(20, 20))
            .write_to(&mut bytes, ImageFormat::Bmp)
            .unwrap();
        assert_eq!(
            read_bytes(bytes.get_ref(), "large.bmp", &opts)
                .unwrap()
                .status,
            "resource_limit"
        );
    }

    #[test]
    fn webp_preflight_rejects_large_embedded_frames_before_decoder_allocation() {
        fn chunk(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut bytes = name.to_vec();
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(payload);
            if payload.len() % 2 != 0 {
                bytes.push(0);
            }
            bytes
        }
        let canvas = chunk(b"VP8X", &[0, 0, 0, 0, 1, 0, 0, 1, 0, 0]); // 2x2 canvas
        let inner = chunk(
            b"VP8 ",
            &[0, 0, 0, 0x9d, 0x01, 0x2a, 0xff, 0x3f, 0xff, 0x3f],
        ); // 16383x16383 frame
        let mut data = b"WEBP".to_vec();
        data.extend(canvas);
        data.extend(inner);
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend(data);
        assert_eq!(
            read_bytes(&bytes, "nested.webp", &options())
                .unwrap()
                .status,
            "resource_limit"
        );
        // The same guard covers nested ANMF frames, even before the first frame is decoded.
        let mut frame = vec![0; 16];
        frame[6] = 1;
        frame[9] = 1;
        frame.extend(chunk(b"VP8L", &[0x2f, 0xff, 0xff, 0xff, 0x0f]));
        let mut data = b"WEBP".to_vec();
        data.extend(chunk(b"ANMF", &frame));
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend(data);
        assert_eq!(
            read_bytes(&bytes, "animation.webp", &options())
                .unwrap()
                .status,
            "resource_limit"
        );
    }

    #[test]
    fn malicious_office_xml_zip_expansion_and_paths_are_rejected() {
        let dtd = "<!DOCTYPE document [<!ENTITY ext SYSTEM 'file:///never-open'>]><document><p><t>&ext;</t></p></document>";
        let bytes = package(&[("word/document.xml", dtd)]);
        assert_eq!(
            read_bytes(&bytes, "external.docx", &options())
                .unwrap()
                .status,
            "corrupt"
        );
        let nested = format!("{}x{}", "<p>".repeat(129), "</p>".repeat(129));
        assert_eq!(
            walk_xml(nested.as_bytes(), |_| Ok(())).unwrap_err().status,
            "resource_limit"
        );
        let bytes = package(&[("../word/document.xml", "<document/>")]);
        assert_eq!(
            read_bytes(&bytes, "traversal.docx", &options())
                .unwrap()
                .status,
            "corrupt"
        );
        let huge = "A".repeat(2 * 1024 * 1024);
        let bytes = package(&[("word/document.xml", &huge)]);
        assert_eq!(
            read_bytes(&bytes, "zip-bomb.docx", &options())
                .unwrap()
                .status,
            "resource_limit"
        );
        assert!(relationships_map(
            b"<Relationships><Relationship Id='r' Target='../../escape'/></Relationships>",
            "ppt"
        )
        .is_err());
    }

    #[test]
    fn unsupported_encrypted_corrupt_and_bounds_are_explicit() {
        assert_eq!(
            read_bytes(b"legacy", "old.doc", &options()).unwrap().status,
            "unsupported"
        );
        assert_eq!(
            read_bytes(
                &[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1],
                "encrypted.docx",
                &options()
            )
            .unwrap()
            .status,
            "encrypted"
        );
        assert_eq!(
            read_bytes(b"broken", "broken.pdf", &options())
                .unwrap()
                .status,
            "corrupt"
        );
        assert_eq!(
            read_bytes(b"broken", "broken.docx", &options())
                .unwrap()
                .status,
            "corrupt"
        );
        assert_eq!(
            read_bytes(b"broken", "broken.png", &options())
                .unwrap()
                .status,
            "corrupt"
        );
        assert_eq!(
            read_bytes(&vec![b'a'; MAX_INPUT_BYTES + 1], "huge.txt", &options())
                .unwrap()
                .status,
            "resource_limit"
        );
        assert!(read_bytes(
            b"test",
            "test.txt",
            &ReadOptions {
                limit: 0,
                ..options()
            }
        )
        .is_err());
        assert!(read_bytes(
            b"test",
            "test.txt",
            &ReadOptions {
                max_bytes: 127,
                ..options()
            }
        )
        .is_err());
        assert!(read_bytes(
            b"test",
            "test.txt",
            &ReadOptions {
                original: true,
                ..options()
            }
        )
        .is_err());
    }
}
