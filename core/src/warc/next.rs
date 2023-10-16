use std::io::Read;

use bstr::{BStr, ByteSlice};
use flate2::read::MultiGzDecoder;

use super::{Metadata, Request, Response, WarcRecord};

macro_rules! define_warc_field {
    ($($ty:ident { $($name:ident: $bytes:literal,)* },)*) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            enum $ty {
                $($name,)*
            }

            impl $ty {
                pub fn parse(bytes: &[u8]) -> Option<Self> {
                    match bytes {
                        $($bytes => Some(Self::$name),)*
                        _ => None,
                    }
                }
                pub const fn byte_str(self) -> &'static [u8] {
                    match self {
                        $(Self::$name => $bytes,)*
                    }
                }
            }
        )*
    };
}

define_warc_field! {
    WarcField {
        WarcType: b"WARC-Type",
        WarcRecordId: b"WARC-Record-ID",
        WarcDate: b"WARC-Date",
        ContentLength: b"Content-Length",
        ContentType: b"Content-Type",
        WarcConcurrentTo: b"WARC-Concurrent-To",
        WarcBlockDigest: b"WARC-Block-Digest",
        WarcPayloadDigest: b"WARC-Payload-Digest",
        WarcIpAddress: b"WARC-IP-Address",
        WarcRefersTo: b"WARC-Refers-To",
        WarcTargetUri: b"WARC-Target-URI",
        WarcTruncated: b"WARC-Truncated",
        WarcWarcinfoId: b"WARC-Warcinfo-ID",
        WarcFilename: b"WARC-Filename",
        WarcProfile: b"WARC-Profile",
        WarcIdentifiedPayloadType: b"WARC-Identified-Payload-Type",
        WarcSegmentOriginId: b"WARC-Segment-Origin-ID",
        WarcSegmentNumber: b"WARC-Segment-Number",
        WarcSegmentTotalLength: b"WARC-Segment-Total-Length",
    },
    WarcType {
        Warcinfo: b"warcinfo",
        Response: b"response",
        Resource: b"resource",
        Request: b"request",
        Metadata: b"metadata",
        Revisit: b"revisit",
        Conversion: b"conversion",
        Continuation: b"continuation",
    },
}

pub fn multi_pass(bytes: &[u8]) -> impl Iterator<Item = WarcRecord> + '_ {
    // let mut buf = vec![0; 1024 * 1024 * 1024];
    let mut buf = vec![0; 1024 * 1024 * 1024 / 2];

    let mut decoder = MultiGzDecoder::new(bytes);

    let mut cls = Vec::with_capacity(1024);
    let mut blocks = Vec::with_capacity(1024);
    let mut failed_start = 0;

    let mut records: Vec<WarcRecord> = Vec::new();

    let mut request: Option<Request> = None;
    let mut response: Option<Response> = None;
    let mut metadata: Option<Metadata> = None;

    std::iter::from_fn(move || {
        let pre = std::time::Instant::now();
        // tracing::debug!(?failed_start);
        let mut cursor = failed_start;
        while cursor < buf.len() {
            let read = decoder.read(&mut buf[cursor..]).ok()?;
            if read == 0 {
                break;
            }
            cursor += read;
        }
        let read_bytes = &buf[0..cursor];
        // buf.binary

        blocks.clear();
        blocks.extend(read_bytes.find_iter(b"\r\n\r\n"));

        let mut last_end = 0;

        failed_start = 0;
        let cl_str = b"Content-Length: ";
        let mut warc_parts = 0;
        cls.clear();
        cls.extend(read_bytes.find_iter(cl_str));
        // read_bytes
        //     .find_iter(cl_str)
        cls.iter()
            .copied()
            .filter_map(|pos| {
                let (pre, _) = read_bytes[cl_str.len() + pos..].split_once_str(b"\r\n")?;
                let len = std::str::from_utf8(pre).unwrap().parse::<usize>().ok()?;
                let (descriptor_start, descriptor_end) = match blocks.binary_search(&pos) {
                    Ok(idx) | Err(idx) => (
                        blocks.get(idx - 1).copied().unwrap_or(0),
                        blocks.get(idx).copied().unwrap_or(buf.len() + 1),
                    ),
                };
                let content_start = descriptor_end + 4;
                let content_end = content_start + len;

                if last_end > content_start {
                    return None;
                }
                last_end = content_end;
                warc_parts += 1;

                let Some(descriptor) = buf.get(descriptor_start..content_start) else {
                    if failed_start == 0 {
                        failed_start = descriptor_start;
                    }
                    return None;
                };
                let descriptor = BStr::new(descriptor);
                let Some(content) = buf.get(content_start..content_end) else {
                    if failed_start == 0 {
                        failed_start = descriptor_start;
                    }
                    return None;
                };
                let content = BStr::new(content);
                let ty = WarcType::parse(read_field(WarcField::WarcType.byte_str(), descriptor)?)?;

                match ty {
                    WarcType::Warcinfo => return None,
                    WarcType::Response => {
                        request.as_ref()?;
                        response = Some(Response {
                            body: decode(content),
                            payload_type: None,
                        });
                    }
                    WarcType::Resource => todo!(),
                    WarcType::Request => {
                        let url =
                            read_field(WarcField::WarcTargetUri.byte_str(), descriptor).unwrap();
                        request = Some(Request {
                            url: url.to_str().unwrap().to_string(),
                        });
                        response = None;
                    }
                    WarcType::Metadata => {
                        let request = request.take()?;
                        let response = response.take()?;
                        let metadata = Metadata {
                            fetch_time_ms: read_field(b"fetchTimeMs", content)?
                                .to_str()
                                .unwrap()
                                .parse()
                                .unwrap(),
                        };
                        records.push(WarcRecord {
                            request,
                            response,
                            metadata,
                        });
                    }
                    WarcType::Revisit => todo!(),
                    WarcType::Conversion => todo!(),
                    WarcType::Continuation => todo!(),
                }

                Some((pos, len, ty, descriptor_start, content_start, content_end))
            })
            .count();

        let avg = pre.elapsed() / warc_parts;
        tracing::debug!(ms_pr_warc=?avg);
        // tracing::debug!(cls=?cls[0..10.min(cls.len())]);
        // tracing::debug!(records=?records[0..10.min(records.len())]);

        // tracing::debug!(read=?read_bytes.len(), cls=cls.len(), blocks=?blocks.len());
        if cursor != buf.len() {
            tracing::debug!("finished");
            return None;
        }
        if failed_start != 0 {
            buf.copy_within(failed_start.., 0);
            failed_start = buf.len() - failed_start;
        }
        Some(std::mem::take(&mut records))
        // todo!()
    })
    .fuse()
    .flatten()
}

fn read_field<'s>(key: &[u8], bytes: &'s [u8]) -> Option<&'s [u8]> {
    Some(
        bytes[bytes.find(key)? + key.len() + 2..]
            .split_once_str(b"\r\n")?
            .0,
    )
}

fn decode(raw: &[u8]) -> String {
    if let Ok(res) = String::from_utf8(raw.to_owned()) {
        res
    } else {
        let encodings = [
            encoding_rs::WINDOWS_1251,
            encoding_rs::GBK,
            encoding_rs::SHIFT_JIS,
            encoding_rs::EUC_JP,
            encoding_rs::EUC_KR,
        ];

        for enc in encodings {
            let (cow, _, had_errors) = enc.decode(raw);
            if !had_errors {
                return cow.to_string();
            }
        }

        String::from_utf8_lossy(raw).to_string()
    }
}
