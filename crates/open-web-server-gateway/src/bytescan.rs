//! バイト列の部分列探索。先頭バイトの探索を open-cpu の `find_byte`
//! (AVX-512BW > AVX2 > NEON > スカラーを実行時に自動選択)で行い、候補位置だけを照合する。
//! 結果は `haystack.windows(n).position(|w| w == needle)` と常に一致する。

pub fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > hay.len() {
        return None;
    }
    let last_start = hay.len() - needle.len();
    let mut from = 0;
    while from <= last_start {
        let p = from + open_cpu::find_byte(&hay[from..=last_start], needle[0])?;
        if &hay[p..p + needle.len()] == needle {
            return Some(p);
        }
        from = p + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_windows_position() {
        let mut s = 0x1234_5678_9abc_def1u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for round in 0..2000 {
            let len = (next() % 400) as usize;
            let hay: Vec<u8> = (0..len).map(|_| b"\r\nab"[(next() % 4) as usize]).collect();
            for needle in [&b"\r\n\r\n"[..], b"\n\n", b"ab", b"\r", b"", b"abab\r"] {
                let want = if needle.is_empty() { Some(0) } else { hay.windows(needle.len()).position(|w| w == needle) };
                assert_eq!(find_subslice(&hay, needle), want, "round={round} needle={needle:?}");
            }
        }
    }

    #[test]
    fn edges() {
        assert_eq!(find_subslice(b"", b"a"), None);
        assert_eq!(find_subslice(b"a", b"ab"), None);
        assert_eq!(find_subslice(b"xx\r\n\r\nbody", b"\r\n\r\n"), Some(2));
    }
}
