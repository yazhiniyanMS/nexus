//! Reads the header of a GGUF model file: its metadata and tensor shapes, never the weights.
//! Everything reported comes from the file itself; nothing is guessed from the file name.
//!
//! Layout (little-endian): magic "GGUF", version u32, tensor count u64, metadata count u64,
//! then metadata key/value pairs, then one description per tensor.

use crate::error::{AppError, Result};
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const MAX_METADATA: u64 = 100_000;
const MAX_TENSORS: u64 = 1_000_000;
/// Longest string kept in memory. Longer values (none are expected) are skipped.
const MAX_STRING: u64 = 4 * 1024 * 1024;
/// Total bytes of header this reader is willing to walk through.
const MAX_HEADER_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct GgufInfo {
    pub version: u32,
    pub tensor_count: u64,
    pub name: Option<String>,
    pub architecture: Option<String>,
    pub basename: Option<String>,
    pub organization: Option<String>,
    pub size_label: Option<String>,
    /// Quantization of most tensors, from `general.file_type`.
    pub quantization: Option<String>,
    pub parameter_count: Option<u64>,
    /// "metadata" when the file states the count, "tensor shapes" when it was summed from them.
    pub parameter_source: Option<&'static str>,
    pub context_length: Option<u64>,
    pub embedding_length: Option<u64>,
    pub block_count: Option<u64>,
    pub license: Option<String>,
    pub url: Option<String>,
    pub has_chat_template: bool,
}

/// Name of a `general.file_type` value (llama.cpp's `llama_ftype`), or None for unknown values.
pub fn file_type_name(value: u32) -> Option<&'static str> {
    Some(match value {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_XXS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        26 => "IQ3_S",
        27 => "IQ3_M",
        28 => "IQ2_S",
        29 => "IQ2_M",
        30 => "IQ4_XS",
        31 => "IQ1_M",
        32 => "BF16",
        36 => "TQ1_0",
        37 => "TQ2_0",
        38 => "MXFP4_MOE",
        39 => "NVFP4",
        40 => "Q1_0",
        41 => "Q2_0",
        42 => "F8_E4M3",
        _ => return None,
    })
}

enum Value {
    Uint(u64),
    Text(String),
    Other,
}

struct Reader<R: Read> {
    inner: R,
    read: u64,
}

impl<R: Read> Reader<R> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut buf = [0u8; N];
        self.inner.read_exact(&mut buf).map_err(|_| truncated())?;
        self.read += N as u64;
        if self.read > MAX_HEADER_BYTES {
            return Err("This GGUF header is larger than NEXUS will read".into());
        }
        Ok(buf)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }

    fn skip(&mut self, mut n: u64) -> Result<()> {
        self.read += n;
        if self.read > MAX_HEADER_BYTES {
            return Err("This GGUF header is larger than NEXUS will read".into());
        }
        let mut sink = [0u8; 8192];
        while n > 0 {
            let take = n.min(sink.len() as u64) as usize;
            self.inner.read_exact(&mut sink[..take]).map_err(|_| truncated())?;
            n -= take as u64;
        }
        Ok(())
    }

    /// A length-prefixed string; None when it is too long to keep (it is skipped).
    fn string(&mut self) -> Result<Option<String>> {
        let len = self.u64()?;
        if len > MAX_STRING {
            self.skip(len)?;
            return Ok(None);
        }
        let mut buf = vec![0u8; len as usize];
        self.inner.read_exact(&mut buf).map_err(|_| truncated())?;
        self.read += len;
        Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
    }

    fn value(&mut self, kind: u32) -> Result<Value> {
        Ok(match kind {
            0 | 1 | 7 => Value::Uint(u64::from(self.bytes::<1>()?[0])),
            2 | 3 => Value::Uint(u64::from(u16::from_le_bytes(self.bytes()?))),
            4 => Value::Uint(u64::from(self.u32()?)),
            // Signed and floating values are never among the fields read here.
            5 | 6 => {
                self.skip(4)?;
                Value::Other
            }
            8 => self.string()?.map(Value::Text).unwrap_or(Value::Other),
            9 => {
                let item = self.u32()?;
                let count = self.u64()?;
                match item {
                    0 | 1 | 7 => self.skip(count)?,
                    2 | 3 => self.skip(count.saturating_mul(2))?,
                    4..=6 => self.skip(count.saturating_mul(4))?,
                    10..=12 => self.skip(count.saturating_mul(8))?,
                    8 => {
                        for _ in 0..count {
                            let len = self.u64()?;
                            self.skip(len)?;
                        }
                    }
                    _ => return Err("This GGUF file uses a metadata type NEXUS does not know".into()),
                }
                Value::Other
            }
            10 => Value::Uint(self.u64()?),
            11 | 12 => {
                self.skip(8)?;
                Value::Other
            }
            _ => return Err("This GGUF file uses a metadata type NEXUS does not know".into()),
        })
    }
}

fn truncated() -> AppError {
    AppError::Msg("The file ends before its GGUF header does (incomplete download?)".into())
}

/// Parse a GGUF header from any reader.
pub fn parse(reader: impl Read) -> Result<GgufInfo> {
    let mut r = Reader { inner: reader, read: 0 };
    if &r.bytes::<4>()? != b"GGUF" {
        return Err("Not a GGUF file".into());
    }
    let version = r.u32()?;
    if !(2..=3).contains(&version) {
        return Err(AppError::Msg(format!("GGUF version {version} is not supported")));
    }
    let tensor_count = r.u64()?;
    let metadata_count = r.u64()?;
    if metadata_count > MAX_METADATA || tensor_count > MAX_TENSORS {
        return Err("This GGUF header is not plausible (too many entries)".into());
    }
    let mut info = GgufInfo { version, tensor_count, ..Default::default() };
    let mut context: Vec<(String, u64)> = vec![];
    let mut embedding: Vec<(String, u64)> = vec![];
    let mut blocks: Vec<(String, u64)> = vec![];
    for _ in 0..metadata_count {
        let key = r.string()?.unwrap_or_default();
        let kind = r.u32()?;
        let value = r.value(kind)?;
        match (key.as_str(), value) {
            ("general.name", Value::Text(v)) => info.name = Some(v),
            ("general.architecture", Value::Text(v)) => info.architecture = Some(v),
            ("general.basename", Value::Text(v)) => info.basename = Some(v),
            ("general.organization", Value::Text(v)) => info.organization = Some(v),
            ("general.size_label", Value::Text(v)) => info.size_label = Some(v),
            ("general.license", Value::Text(v)) => info.license = Some(v),
            ("general.url", Value::Text(v)) => info.url = Some(v),
            ("general.file_type", Value::Uint(v)) => info.quantization = u32::try_from(v).ok().and_then(file_type_name).map(String::from),
            ("general.parameter_count", Value::Uint(v)) => {
                info.parameter_count = Some(v);
                info.parameter_source = Some("metadata");
            }
            ("tokenizer.chat_template", _) => info.has_chat_template = true,
            (k, Value::Uint(v)) if k.ends_with(".context_length") => context.push((k.to_string(), v)),
            (k, Value::Uint(v)) if k.ends_with(".embedding_length") => embedding.push((k.to_string(), v)),
            (k, Value::Uint(v)) if k.ends_with(".block_count") => blocks.push((k.to_string(), v)),
            _ => {}
        }
    }
    // Architecture-specific keys are named "<architecture>.<field>".
    let own = |found: Vec<(String, u64)>, field: &str| {
        let wanted = info.architecture.as_ref().map(|a| format!("{a}.{field}"));
        found.iter().find(|(k, _)| Some(k) == wanted.as_ref()).or(found.first()).map(|(_, v)| *v)
    };
    info.context_length = own(context, "context_length");
    info.embedding_length = own(embedding, "embedding_length");
    info.block_count = own(blocks, "block_count");

    // Without a stated parameter count, add up the tensor shapes. A header that stops short of
    // them (it should not) simply leaves the count unknown.
    if info.parameter_count.is_none() && tensor_count > 0 {
        let mut total: u64 = 0;
        let mut complete = true;
        for _ in 0..tensor_count {
            let shape = (|| -> Result<u64> {
                let name_len = r.u64()?;
                r.skip(name_len)?;
                let dims = r.u32()?;
                if dims > 8 {
                    return Err("implausible tensor".into());
                }
                let mut elements: u64 = 1;
                for _ in 0..dims {
                    elements = elements.saturating_mul(r.u64()?);
                }
                r.skip(4 + 8)?; // tensor type, data offset
                Ok(elements)
            })();
            match shape {
                Ok(n) => total = total.saturating_add(n),
                Err(_) => {
                    complete = false;
                    break;
                }
            }
        }
        if complete && total > 0 {
            info.parameter_count = Some(total);
            info.parameter_source = Some("tensor shapes");
        }
    }
    Ok(info)
}

/// Read the header of a GGUF file on disk.
pub fn read(path: &Path) -> Result<GgufInfo> {
    parse(BufReader::with_capacity(64 * 1024, File::open(path)?))
}

/// Builds GGUF headers for tests.
#[cfg(test)]
pub mod testfile {
    pub enum Field<'a> {
        Text(&'a str, &'a str),
        U32(&'a str, u32),
        U64(&'a str, u64),
        Strings(&'a str, &'a [&'a str]),
    }

    fn string(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u64).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }

    /// A GGUF v3 header with the given metadata and tensors (name, dimensions), followed by padding.
    pub fn build(fields: &[Field], tensors: &[(&str, &[u64])]) -> Vec<u8> {
        let mut out = b"GGUF".to_vec();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&(tensors.len() as u64).to_le_bytes());
        out.extend_from_slice(&(fields.len() as u64).to_le_bytes());
        for field in fields {
            match field {
                Field::Text(k, v) => {
                    string(&mut out, k);
                    out.extend_from_slice(&8u32.to_le_bytes());
                    string(&mut out, v);
                }
                Field::U32(k, v) => {
                    string(&mut out, k);
                    out.extend_from_slice(&4u32.to_le_bytes());
                    out.extend_from_slice(&v.to_le_bytes());
                }
                Field::U64(k, v) => {
                    string(&mut out, k);
                    out.extend_from_slice(&10u32.to_le_bytes());
                    out.extend_from_slice(&v.to_le_bytes());
                }
                Field::Strings(k, items) => {
                    string(&mut out, k);
                    out.extend_from_slice(&9u32.to_le_bytes());
                    out.extend_from_slice(&8u32.to_le_bytes());
                    out.extend_from_slice(&(items.len() as u64).to_le_bytes());
                    for item in *items {
                        string(&mut out, item);
                    }
                }
            }
        }
        for (name, dims) in tensors {
            string(&mut out, name);
            out.extend_from_slice(&(dims.len() as u32).to_le_bytes());
            for d in *dims {
                out.extend_from_slice(&d.to_le_bytes());
            }
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&0u64.to_le_bytes());
        }
        out.extend_from_slice(&[0u8; 64]);
        out
    }

    /// A small, valid model header used wherever a test needs "a GGUF file".
    pub fn sample() -> Vec<u8> {
        build(
            &[
                Field::Text("general.architecture", "llama"),
                Field::Text("general.name", "Tiny Test"),
                Field::U32("general.file_type", 15),
                Field::U64("llama.context_length", 2048),
            ],
            &[("w", &[16, 4])],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testfile::{build, Field};
    use super::*;

    #[test]
    fn reads_metadata_and_skips_large_arrays() {
        let bytes = build(
            &[
                Field::Text("general.architecture", "llama"),
                Field::Text("general.name", "Tiny Stories 15M"),
                Field::Strings("tokenizer.ggml.tokens", &["<s>", "</s>", "hello", "world"]),
                Field::U32("general.file_type", 15),
                Field::U32("clip.context_length", 77),
                Field::U64("llama.context_length", 4096),
                Field::U32("llama.embedding_length", 288),
                Field::U32("llama.block_count", 6),
                Field::Text("general.license", "mit"),
                Field::Text("tokenizer.chat_template", "{{ messages }}"),
                Field::U64("general.parameter_count", 15_000_000),
            ],
            &[("token_embd.weight", &[288, 32000])],
        );
        let info = parse(&bytes[..]).unwrap();
        assert_eq!(info.version, 3);
        assert_eq!(info.name.as_deref(), Some("Tiny Stories 15M"));
        assert_eq!(info.architecture.as_deref(), Some("llama"));
        assert_eq!(info.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(info.context_length, Some(4096), "the architecture's own key wins over another model's");
        assert_eq!((info.embedding_length, info.block_count), (Some(288), Some(6)));
        assert_eq!(info.license.as_deref(), Some("mit"));
        assert!(info.has_chat_template);
        assert_eq!((info.parameter_count, info.parameter_source), (Some(15_000_000), Some("metadata")));
    }

    #[test]
    fn counts_parameters_from_tensor_shapes_when_the_file_does_not_state_them() {
        let bytes = build(&[Field::Text("general.architecture", "llama")], &[("a", &[10, 20]), ("b", &[7]), ("c", &[2, 3, 4])]);
        let info = parse(&bytes[..]).unwrap();
        assert_eq!((info.parameter_count, info.parameter_source), (Some(200 + 7 + 24), Some("tensor shapes")));
        assert_eq!(info.quantization, None, "nothing is guessed when the file does not say");
        assert_eq!(info.context_length, None);
        assert!(!info.has_chat_template);
    }

    #[test]
    fn rejects_files_that_are_not_gguf_or_are_cut_short() {
        assert!(parse(&b"GGML\x03\x00\x00\x00"[..]).unwrap_err().to_string().contains("Not a GGUF"));
        assert!(parse(&b"GG"[..]).is_err());
        let full = build(&[Field::Text("general.name", "x"), Field::U64("llama.context_length", 1)], &[]);
        let cut = &full[..full.len() - 64 - 6];
        assert!(parse(cut).unwrap_err().to_string().contains("incomplete"));
        // An absurd entry count is refused instead of being walked.
        let mut bogus = b"GGUF".to_vec();
        bogus.extend_from_slice(&3u32.to_le_bytes());
        bogus.extend_from_slice(&0u64.to_le_bytes());
        bogus.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(parse(&bogus[..]).unwrap_err().to_string().contains("plausible"));
        let mut v1 = b"GGUF".to_vec();
        v1.extend_from_slice(&1u32.to_le_bytes());
        v1.extend_from_slice(&[0u8; 16]);
        assert!(parse(&v1[..]).unwrap_err().to_string().contains("version 1"));
    }

    #[test]
    fn file_type_names_follow_llama_cpp() {
        assert_eq!(file_type_name(1), Some("F16"));
        assert_eq!(file_type_name(18), Some("Q6_K"));
        assert_eq!(file_type_name(32), Some("BF16"));
        assert_eq!(file_type_name(4), None, "removed values stay unknown");
        assert_eq!(file_type_name(1024), None);
    }
}
