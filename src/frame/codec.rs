use bytes::{BigEndian, BufMut, ByteOrder, BytesMut};
use error::DecodeError;
use frame::{header::{Flags, Len, RawHeader, Type, Version}, Body, RawFrame};
use std::io;
use stream::StreamId;
use tokio_codec::{BytesCodec, Decoder, Encoder};


#[derive(Debug)]
pub struct FrameCodec {
    header_codec: HeaderCodec,
    body_codec: BytesCodec
}

impl FrameCodec {
    pub fn new() -> FrameCodec {
        FrameCodec {
            header_codec: HeaderCodec::new(),
            body_codec: BytesCodec::new()
        }
    }
}

impl Encoder for FrameCodec {
    type Item = RawFrame;
    type Error = io::Error;

    fn encode(&mut self, frame: Self::Item, bytes: &mut BytesMut) -> Result<(), Self::Error> {
        self.header_codec.encode(frame.header, bytes)?;
        self.body_codec.encode(frame.body.0, bytes)
    }
}

impl Decoder for FrameCodec {
    type Item = RawFrame;
    type Error = DecodeError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let header =
            if let Some(header) = self.header_codec.decode(src)? {
                header
            } else {
                return Ok(None)
            };
        if header.typ != Type::Data || header.length.0 == 0 {
            return Ok(Some(RawFrame { header, body: Body::empty() }))
        }
        let len = header.length.0 as usize;
        if src.len() < len {
            return Ok(None)
        }
        if let Some(b) = self.body_codec.decode(&mut src.split_to(len))? {
            Ok(Some(RawFrame { header, body: Body(b.freeze()) }))
        } else {
            Ok(None)
        }
    }
}


#[derive(Debug)]
pub struct HeaderCodec(());

impl HeaderCodec {
    pub fn new() -> HeaderCodec {
        HeaderCodec(())
    }
}

impl Encoder for HeaderCodec {
    type Item = RawHeader;
    type Error = io::Error;

    fn encode(&mut self, hdr: Self::Item, bytes: &mut BytesMut) -> Result<(), Self::Error> {
        bytes.reserve(12);
        bytes.put_u8(hdr.version.0);
        bytes.put_u8(hdr.typ as u8);
        bytes.put_u16_be(hdr.flags.0);
        bytes.put_u32_be(hdr.stream_id.as_u32());
        bytes.put_u32_be(hdr.length.0);
        Ok(())
    }
}

impl Decoder for HeaderCodec {
    type Item = RawHeader;
    type Error = DecodeError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 12 {
            return Ok(None)
        }
        let src = src.split_to(12);
        let header = RawHeader {
            version: Version(src[0]),
            typ: match src[1] {
                0 => Type::Data,
                1 => Type::WindowUpdate,
                2 => Type::Ping,
                3 => Type::GoAway,
                t => return Err(DecodeError::Type(t))
            },
            flags: Flags(BigEndian::read_u16(&src[2..4])),
            stream_id: StreamId::new(BigEndian::read_u32(&src[4..8])),
            length: Len(BigEndian::read_u32(&src[8..12]))
        };
        Ok(Some(header))
    }
}


#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use quickcheck::{Arbitrary, Gen, quickcheck};
    use super::*;

    impl Arbitrary for RawFrame {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            use frame::header::Type::*;
            let ty = g.choose(&[Data, WindowUpdate, Ping, GoAway]).unwrap().clone();
            let len = g.gen::<u16>() as u32;
            let header = RawHeader {
                version: Version(g.gen()),
                typ: ty,
                flags: Flags(g.gen()),
                stream_id: StreamId::new(g.gen()),
                length: Len(len)
            };
            let body =
                if ty == Type::Data {
                    let bytes = Bytes::from(vec![0; len as usize]);
                    Body::from_bytes(bytes).unwrap()
                } else {
                    Body::empty()
                };
            RawFrame { header, body }
        }
    }

    #[test]
    fn frame_identity() {
        fn property(f: RawFrame) -> bool {
            let mut buf = BytesMut::with_capacity(12 + f.body.bytes().len());
            let mut codec = FrameCodec::new();
            if codec.encode(f.clone(), &mut buf).is_err() {
                return false
            }
            if let Ok(x) = codec.decode(&mut buf) {
                x == Some(f)
            } else {
                false
            }
        }
        quickcheck(property as fn(RawFrame) -> bool)
    }
}

