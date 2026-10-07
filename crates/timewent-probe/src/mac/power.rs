//! IOKit power assertions: one `IOPMCopyAssertionsStatus` call per sample. It returns the
//! system-wide level per assertion type (a single dictionary from powerd), which is all
//! `media_active` needs — cheaper than listing assertions per process. No permission needed.

use std::ptr;

use objc2_core_foundation::{
    CFArray, CFDate, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};
use objc2_io_kit::{kIOReturnSuccess, IOPMCopyAssertionsByProcess, IOPMCopyAssertionsStatus};

use crate::media::display_kept_awake;

pub(super) fn media_active() -> bool {
    let mut raw: *const CFDictionary = ptr::null();
    // SAFETY: `raw` is a valid out-pointer; on success it receives a +1 dictionary.
    let rc = unsafe { IOPMCopyAssertionsStatus(&mut raw) };
    // Take ownership first so the dictionary is released on every path (Create/Copy rule).
    // SAFETY: a non-null out value is an owned (+1) CFDictionary.
    let Some(dict) = ptr::NonNull::new(raw.cast_mut()).map(|d| unsafe { CFRetained::from_raw(d) })
    else {
        return false;
    };
    if rc != kIOReturnSuccess {
        return false;
    }
    // SAFETY: documented shape: assertion-type CFString → CFNumber level.
    let dict: CFRetained<CFDictionary<CFString, CFType>> =
        unsafe { CFRetained::cast_unchecked(dict) };
    display_kept_awake(|t| {
        dict.get(&CFString::from_str(t))
            .and_then(|v| v.downcast::<CFNumber>().ok())
            .and_then(|n| n.as_i64())
    })
}

/// Processes playing audio right now: `(pid, assertion start)` for every assertion whose
/// `ResourcesUsed` includes `audio-out` (decision Q1). coreaudiod owns these; the player is
/// the on-behalf-of pid. One `IOPMCopyAssertionsByProcess` call (~0.1ms measured).
pub(super) fn audio_holders() -> Vec<(i32, f64)> {
    let mut raw: *const CFDictionary = ptr::null();
    // SAFETY: valid out-pointer; on success a +1 dictionary.
    let rc = unsafe { IOPMCopyAssertionsByProcess(&mut raw) };
    // SAFETY: a non-null out value is owned (+1), released by Drop on every path.
    let Some(dict) = ptr::NonNull::new(raw.cast_mut()).map(|d| unsafe { CFRetained::from_raw(d) })
    else {
        return Vec::new();
    };
    if rc != kIOReturnSuccess {
        return Vec::new();
    }
    // SAFETY: documented shape — pid (CFNumber) → CFArray of assertion dictionaries.
    let dict: CFRetained<CFDictionary<CFNumber, CFArray<CFDictionary<CFString, CFType>>>> =
        unsafe { CFRetained::cast_unchecked(dict) };
    let resources = CFString::from_static_str("ResourcesUsed");
    let started_key = CFString::from_static_str("AssertStartWhen");
    let (owners, lists) = dict.to_vecs();
    let mut out = Vec::new();
    for (owner, list) in owners.iter().zip(lists.iter()) {
        for a in list.iter() {
            let plays = a
                .get(&resources)
                .and_then(|v| v.downcast::<CFArray>().ok())
                .is_some_and(|arr| {
                    // SAFETY: ResourcesUsed is an array of CFStrings.
                    let arr: CFRetained<CFArray<CFString>> =
                        unsafe { CFRetained::cast_unchecked(arr) };
                    arr.iter().any(|r| r.to_string() == "audio-out")
                });
            if !plays {
                continue;
            }
            let (keys, values) = a.to_vecs();
            let on_behalf = keys
                .iter()
                .zip(values.iter())
                .find(|(k, _)| k.to_string().ends_with("OnBehalfOfPID"))
                .and_then(|(_, v)| v.clone().downcast::<CFNumber>().ok())
                .and_then(|n| n.as_i64());
            let Some(pid) = on_behalf.or_else(|| owner.as_i64()) else {
                continue;
            };
            let started = a
                .get(&started_key)
                .and_then(|v| v.downcast::<CFDate>().ok())
                .map_or(0.0, |d| d.absolute_time());
            if let Ok(pid) = i32::try_from(pid) {
                out.push((pid, started));
            }
        }
    }
    out
}

#[cfg(test)]
mod dump {
    use super::*;

    /// Cost of one IOPMCopyAssertionsByProcess call, incl. walking the result. Opt-in.
    #[test]
    #[ignore]
    fn assertions_by_process_cost() {
        let n = 200;
        let t = std::time::Instant::now();
        let mut entries = 0;
        for _ in 0..n {
            let mut raw: *const CFDictionary = ptr::null();
            let rc = unsafe { IOPMCopyAssertionsByProcess(&mut raw) };
            assert_eq!(rc, kIOReturnSuccess);
            let dict =
                unsafe { CFRetained::from_raw(ptr::NonNull::new(raw.cast_mut()).expect("dict")) };
            let dict: CFRetained<CFDictionary<CFNumber, CFArray<CFDictionary<CFString, CFType>>>> =
                unsafe { CFRetained::cast_unchecked(dict) };
            let (_, lists) = dict.to_vecs();
            entries = lists.iter().map(|l| l.len()).sum::<usize>();
        }
        println!(
            "IOPMCopyAssertionsByProcess: {:?} per call ({entries} assertions)",
            t.elapsed() / n
        );
    }

    /// While something plays (e.g. `afplay`), its pid shows up. Opt-in: real OS.
    #[test]
    #[ignore]
    fn audio_holders_names_the_player() {
        let h = audio_holders();
        println!("audio holders: {h:?}");
        for (pid, _) in &h {
            println!("  {pid}: {:?}", super::super::workspace::app_of_pid(*pid));
        }
    }

    /// Prints every assertion dict (dev aid while audio plays). Opt-in.
    #[test]
    #[ignore]
    fn dump_assertions_by_process() {
        let mut raw: *const CFDictionary = ptr::null();
        let rc = unsafe { IOPMCopyAssertionsByProcess(&mut raw) };
        assert_eq!(rc, kIOReturnSuccess);
        let dict =
            unsafe { CFRetained::from_raw(ptr::NonNull::new(raw.cast_mut()).expect("dict")) };
        let dict: CFRetained<CFDictionary<CFNumber, CFArray<CFDictionary<CFString, CFType>>>> =
            unsafe { CFRetained::cast_unchecked(dict) };
        let (pids, lists) = dict.to_vecs();
        for (pid, list) in pids.iter().zip(lists.iter()) {
            for a in list.iter() {
                let (ks, vs) = a.to_vecs();
                let kv: Vec<String> = ks
                    .iter()
                    .zip(vs.iter())
                    .map(|(k, v)| format!("{k}={v:?}"))
                    .collect();
                println!("pid {:?}: {}", pid.as_i64(), kv.join(" | "));
            }
        }
    }
}
