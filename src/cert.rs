//! What a server's certificate says about itself: who it is for, who
//! issued it and until when. Read from the DER rustls hands over, with a
//! reader for just the parts of X.509 that are shown: the validity, the
//! common name and organisation of the subject and the issuer, and the DNS
//! names. Anything it doesn't understand is left out rather than failing.

/// The parts of a certificate `pepe ping` shows
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cert {
    /// The subject's common name, as a browser would show it
    pub subject: String,
    /// The issuer's organisation, or its common name when it has none
    pub issuer: String,
    /// Seconds since the epoch
    pub not_before: i64,
    pub not_after: i64,
    /// The DNS names it covers, in the order they are listed
    pub names: Vec<String>,
}

impl Cert {
    /// Whole days until it expires at `now`; negative once it has
    pub fn days_left(&self, now: i64) -> i64 {
        (self.not_after - now).div_euclid(86_400)
    }

    /// Whether it is good at `now`
    pub fn valid_at(&self, now: i64) -> bool {
        self.not_before <= now && now < self.not_after
    }

    /// `expires in 61 days`, `expires today`, `expired 3 days ago`
    pub fn expiry(&self, now: i64) -> String {
        match self.days_left(now) {
            d if d < -1 => format!("expired {} days ago", -d),
            -1 => "expired yesterday".into(),
            0 => "expires today".into(),
            1 => "expires tomorrow".into(),
            d => format!("expires in {d} days"),
        }
    }
}

/// A DER element: its tag, its content, and what follows it
struct Tlv<'a> {
    tag: u8,
    content: &'a [u8],
    rest: &'a [u8],
}

/// Read one element from the front of `buf`
fn tlv(buf: &[u8]) -> Option<Tlv<'_>> {
    let (&tag, after_tag) = buf.split_first()?;
    let (&first, after_len) = after_tag.split_first()?;
    let (len, body) = if first < 0x80 {
        (usize::from(first), after_len)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 || after_len.len() < n {
            return None;
        }
        let len = after_len[..n]
            .iter()
            .fold(0usize, |acc, &b| (acc << 8) | usize::from(b));
        (len, &after_len[n..])
    };
    if body.len() < len {
        return None;
    }
    Some(Tlv {
        tag,
        content: &body[..len],
        rest: &body[len..],
    })
}

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const OID: u8 = 0x06;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
/// `[0] EXPLICIT`: the version, when a certificate says one
const VERSION: u8 = 0xa0;
/// `[3] EXPLICIT`: the extensions
const EXTENSIONS: u8 = 0xa3;
/// `[2] IMPLICIT IA5String`: a dNSName in a subjectAltName
const DNS_NAME: u8 = 0x82;
const COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
const ORGANIZATION: &[u8] = &[0x55, 0x04, 0x0a];
const SUBJECT_ALT_NAME: &[u8] = &[0x55, 0x1d, 0x11];

/// Read what is shown of the certificate in `der`; None when it isn't one
pub fn parse(der: &[u8]) -> Option<Cert> {
    let certificate = tlv(der).filter(|t| t.tag == SEQUENCE)?;
    let tbs = tlv(certificate.content).filter(|t| t.tag == SEQUENCE)?;
    let mut fields = tbs.content;
    // The version is optional; everything after it is in a fixed order
    let mut next = tlv(fields)?;
    if next.tag == VERSION {
        fields = next.rest;
        next = tlv(fields)?;
    }
    let _serial = next;
    let signature = tlv(_serial.rest).filter(|t| t.tag == SEQUENCE)?;
    let issuer = tlv(signature.rest).filter(|t| t.tag == SEQUENCE)?;
    let validity = tlv(issuer.rest).filter(|t| t.tag == SEQUENCE)?;
    let subject = tlv(validity.rest).filter(|t| t.tag == SEQUENCE)?;
    let not_before = tlv(validity.content)?;
    let not_after = tlv(not_before.rest)?;
    let (subject_cn, _) = name(subject.content);
    let (issuer_cn, issuer_o) = name(issuer.content);
    let mut cert = Cert {
        subject: subject_cn.unwrap_or_default(),
        issuer: issuer_o.or(issuer_cn).unwrap_or_default(),
        not_before: time(not_before.tag, not_before.content)?,
        not_after: time(not_after.tag, not_after.content)?,
        names: Vec::new(),
    };
    // After the subject: the public key, then optional unique ids and
    // the extensions, which are the only [3]
    let mut rest = subject.rest;
    while let Some(element) = tlv(rest) {
        if element.tag == EXTENSIONS {
            cert.names = dns_names(element.content);
            break;
        }
        rest = element.rest;
    }
    if cert.subject.is_empty() {
        cert.subject = cert.names.first().cloned().unwrap_or_default();
    }
    Some(cert)
}

/// A Name's common name and organisation, when it has them
fn name(content: &[u8]) -> (Option<String>, Option<String>) {
    let (mut cn, mut o) = (None, None);
    let mut rdns = content;
    while let Some(rdn) = tlv(rdns).filter(|t| t.tag == SET) {
        let mut attributes = rdn.content;
        while let Some(attribute) = tlv(attributes).filter(|t| t.tag == SEQUENCE) {
            if let Some(oid) = tlv(attribute.content).filter(|t| t.tag == OID) {
                if let Some(value) = tlv(oid.rest) {
                    let text = String::from_utf8_lossy(value.content).into_owned();
                    if oid.content == COMMON_NAME {
                        cn.get_or_insert(text);
                    } else if oid.content == ORGANIZATION {
                        o.get_or_insert(text);
                    }
                }
            }
            attributes = attribute.rest;
        }
        rdns = rdn.rest;
    }
    (cn, o)
}

/// The dNSNames of the subjectAltName among `extensions`
fn dns_names(extensions: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let Some(list) = tlv(extensions).filter(|t| t.tag == SEQUENCE) else {
        return names;
    };
    let mut items = list.content;
    while let Some(extension) = tlv(items).filter(|t| t.tag == SEQUENCE) {
        items = extension.rest;
        let Some(oid) = tlv(extension.content).filter(|t| t.tag == OID) else {
            continue;
        };
        if oid.content != SUBJECT_ALT_NAME {
            continue;
        }
        // `critical` is optional before the OCTET STRING that holds the value
        let mut value = tlv(oid.rest);
        if let Some(v) = &value {
            if v.tag == 0x01 {
                value = tlv(v.rest);
            }
        }
        let Some(octets) = value.filter(|t| t.tag == 0x04) else {
            continue;
        };
        let Some(general_names) = tlv(octets.content).filter(|t| t.tag == SEQUENCE) else {
            continue;
        };
        let mut entries = general_names.content;
        while let Some(entry) = tlv(entries) {
            if entry.tag == DNS_NAME {
                names.push(String::from_utf8_lossy(entry.content).into_owned());
            }
            entries = entry.rest;
        }
    }
    names
}

/// A UTCTime (`YYMMDDHHMMSSZ`) or GeneralizedTime (`YYYYMMDDHHMMSSZ`) as
/// seconds since the epoch
fn time(tag: u8, text: &[u8]) -> Option<i64> {
    let digit = |b: u8| b.is_ascii_digit().then(|| i64::from(b - b'0'));
    let number =
        |s: &[u8]| -> Option<i64> { s.iter().try_fold(0i64, |n, &b| Some(n * 10 + digit(b)?)) };
    let (year, rest) = match tag {
        UTC_TIME if text.len() >= 12 => {
            let yy = number(&text[..2])?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &text[2..])
        }
        GENERALIZED_TIME if text.len() >= 14 => (number(&text[..4])?, &text[4..]),
        _ => return None,
    };
    let month = number(&rest[..2])? as u32;
    let day = number(&rest[2..4])? as u32;
    let hour = number(&rest[4..6])?;
    let minute = number(&rest[6..8])?;
    let second = number(&rest[8..10])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Days since the epoch of a calendar date (Howard Hinnant's algorithm)
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A self-signed certificate for ping.test, good from 2026-10-09
    /// 23:06:40 UTC for a year, with two DNS names
    const PING_TEST: &str = "308201cb30820170a0030201020214100ceb8d88bd115c9f39a887369f0499e6fe055d300a06082a8648ce3d04030230\
283112301006035504030c0970696e672e7465737431123010060355040a0c09506570652054657374301e170d323631\
3030393233303634305a170d3237313030393233303634305a30283112301006035504030c0970696e672e7465737431\
123010060355040a0c095065706520546573743059301306072a8648ce3d020106082a8648ce3d03010703420004c421\
1f2d8d37f4e8294d662f4419d8eb736b7823ec3968d5e92784121a6e04e9771e5af91e024deec08b1d466006df57e04c\
c4b2b2b54c910bbfb51b3627f14ba3783076301d0603551d0e0416041424116e9a5e2b1c9d0c3bb34ab6274ff1127a5f\
21301f0603551d2304183016801424116e9a5e2b1c9d0c3bb34ab6274ff1127a5f21300f0603551d130101ff04053003\
0101ff30230603551d11041c301a820970696e672e74657374820d7777772e70696e672e74657374300a06082a8648ce\
3d04030203490030460221009aefb83ddf13a6a2d0fcb25695bb804c3b72538ff5555bea0cf934740453c3a0022100f9\
18b6d24efc1fa5b9eba3fc0d7234bff065861e8834a2959121538b1084388f";

    fn der() -> Vec<u8> {
        (0..PING_TEST.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&PING_TEST[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn reads_the_names_and_the_validity() {
        let cert = parse(&der()).unwrap();
        assert_eq!(cert.subject, "ping.test");
        assert_eq!(cert.issuer, "Pepe Test");
        assert_eq!(cert.names, ["ping.test", "www.ping.test"]);
        // 2026-10-09T23:06:40Z and a year later
        assert_eq!(cert.not_before, 1_791_587_200);
        assert_eq!(cert.not_after, 1_823_123_200);
        assert_eq!(cert.days_left(cert.not_before), 365);
        assert_eq!(
            cert.expiry(cert.not_before + 86_400 * 300),
            "expires in 65 days"
        );
        assert_eq!(cert.expiry(cert.not_after), "expires today");
        assert_eq!(
            cert.expiry(cert.not_after + 86_400 * 3),
            "expired 3 days ago"
        );
        assert!(cert.valid_at(cert.not_before + 1));
        assert!(!cert.valid_at(cert.not_after));
    }

    #[test]
    fn times_in_both_forms() {
        assert_eq!(time(UTC_TIME, b"700101000000Z"), Some(0));
        assert_eq!(time(UTC_TIME, b"491231235959Z"), Some(2_524_607_999));
        assert_eq!(
            time(GENERALIZED_TIME, b"20500101000000Z"),
            Some(2_524_608_000)
        );
        assert_eq!(time(UTC_TIME, b"7001"), None);
        assert_eq!(time(UTC_TIME, b"701301000000Z"), None);
    }

    #[test]
    fn what_isnt_a_certificate_is_none() {
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"\x30\x03\x02\x01\x01"), None);
        let mut cut = der();
        cut.truncate(100);
        assert_eq!(parse(&cut), None);
    }
}
