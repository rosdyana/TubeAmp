// macOS Core Audio process tap for TubeAmp.
//
// Lifecycle (driven from Rust in macos.rs, all calls on the main thread):
//   ta_create(owner, verbose, &sr)  finds the processes we own (our pid, plus WebKit
//                                   processes we are responsible for), creates a muted
//                                   process tap and a private aggregate device, and
//                                   reports the device sample rate. No IOProc runs yet,
//                                   so the caller can build the DSP for `sr` first.
//   ta_start(cb, user)              installs the IOProc and starts the device. `cb` runs on
//                                   the Core Audio thread with the tapped input and the
//                                   output buffer; the caller processes in place.
//   ta_stop()                       stops the device and destroys everything. Idempotent.
//                                   When it returns, `cb` will not run again.
//
// Based on the M0 spike (spikes/m0/src/tap.m); see docs/M0-FINDINGS.md.

#import <CoreAudio/CoreAudio.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#import <Foundation/Foundation.h>

extern int responsibility_get_pid_responsible_for_pid(pid_t);

typedef void (*ta_cb)(const float *in, uint32_t in_ch, float *out, uint32_t out_ch, uint32_t frames, void *user);

static AudioObjectID g_tap = kAudioObjectUnknown;
static AudioObjectID g_agg = kAudioObjectUnknown;
static AudioDeviceIOProcID g_proc = NULL;

static OSStatus get_prop(AudioObjectID obj, AudioObjectPropertySelector sel, UInt32 *size, void *out) {
    AudioObjectPropertyAddress a = {sel, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    return AudioObjectGetPropertyData(obj, &a, 0, NULL, size, out);
}

static NSString *get_str(AudioObjectID obj, AudioObjectPropertySelector sel) {
    CFStringRef s = NULL; UInt32 sz = sizeof(s);
    if (get_prop(obj, sel, &sz, &s) != noErr || !s) return nil;
    return (__bridge_transfer NSString *)s;
}

/// Collects the process objects that belong to us. Returns the count (may be 0).
static int owned_processes(pid_t owner, AudioObjectID *out, int cap, int verbose) {
    AudioObjectPropertyAddress a = {kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 sz = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &a, 0, NULL, &sz) != noErr || sz == 0) return 0;
    int n = sz / sizeof(AudioObjectID);
    AudioObjectID *ids = malloc(sz);
    if (!ids) return 0;
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &a, 0, NULL, &sz, ids) != noErr) { free(ids); return 0; }
    int found = 0;
    int owner_resp = responsibility_get_pid_responsible_for_pid(owner);
    for (int i = 0; i < n; i++) {
        pid_t pid = -1; UInt32 psz = sizeof(pid);
        get_prop(ids[i], kAudioProcessPropertyPID, &psz, &pid);
        UInt32 running = 0, rsz = sizeof(running);
        get_prop(ids[i], kAudioProcessPropertyIsRunningOutput, &rsz, &running);
        int resp = responsibility_get_pid_responsible_for_pid(pid);
        NSString *bid = get_str(ids[i], kAudioProcessPropertyBundleID);
        int webkit = bid && [bid hasPrefix:@"com.apple.WebKit"];
        int mine = pid != owner && (resp == owner || (resp == owner_resp && webkit));
        if (verbose && (mine || running))
            fprintf(stderr, "[tap] proc obj=%u pid=%d resp=%d out=%u %s%s\n", ids[i], pid, resp, running,
                    bid ? bid.UTF8String : "?", mine ? "  <- ours" : "");
        if (mine && found < cap) out[found++] = ids[i];
    }
    free(ids);
    return found;
}

static void destroy_all(void) {
    if (g_proc) {
        AudioDeviceStop(g_agg, g_proc);
        AudioDeviceDestroyIOProcID(g_agg, g_proc);
        g_proc = NULL;
    }
    if (g_agg != kAudioObjectUnknown) { AudioHardwareDestroyAggregateDevice(g_agg); g_agg = kAudioObjectUnknown; }
    if (g_tap != kAudioObjectUnknown) { AudioHardwareDestroyProcessTap(g_tap); g_tap = kAudioObjectUnknown; }
}

/// Returns 0 on success, -10 when no owned process exists yet (retry later), other
/// negative codes for Core Audio failures. On failure nothing is left allocated.
int ta_create(int owner, int verbose, double *out_sr) {
    if (g_tap != kAudioObjectUnknown || g_agg != kAudioObjectUnknown) return -1;
    @autoreleasepool {
        AudioObjectID ids[16];
        int n = owned_processes(owner, ids, 16, verbose);
        if (n <= 0) return -10;
        NSMutableArray<NSNumber *> *procs = [NSMutableArray array];
        for (int i = 0; i < n; i++) [procs addObject:@(ids[i])];

        CATapDescription *desc = [[CATapDescription alloc] initStereoMixdownOfProcesses:procs];
        desc.name = @"TubeAmp tap";
        desc.privateTap = YES;
        desc.muteBehavior = CATapMutedWhenTapped;
        OSStatus err = AudioHardwareCreateProcessTap(desc, &g_tap);
        if (err != noErr) {
            if (verbose) fprintf(stderr, "[tap] CreateProcessTap err=%d\n", (int)err);
            destroy_all();
            return -11;
        }

        AudioObjectID out_dev = kAudioObjectUnknown; UInt32 sz = sizeof(out_dev);
        get_prop(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultOutputDevice, &sz, &out_dev);
        NSString *out_uid = get_str(out_dev, kAudioDevicePropertyDeviceUID);
        if (!out_uid) { destroy_all(); return -12; }

        NSDictionary *agg = @{
            @kAudioAggregateDeviceNameKey: @"TubeAmp",
            @kAudioAggregateDeviceUIDKey: [[NSUUID UUID] UUIDString],
            @kAudioAggregateDeviceMainSubDeviceKey: out_uid,
            @kAudioAggregateDeviceIsPrivateKey: @YES,
            @kAudioAggregateDeviceIsStackedKey: @NO,
            @kAudioAggregateDeviceTapAutoStartKey: @YES,
            @kAudioAggregateDeviceSubDeviceListKey: @[ @{ @kAudioSubDeviceUIDKey: out_uid } ],
            @kAudioAggregateDeviceTapListKey: @[ @{ @kAudioSubTapUIDKey: desc.UUID.UUIDString, @kAudioSubTapDriftCompensationKey: @YES } ],
        };
        err = AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)agg, &g_agg);
        if (err != noErr) {
            if (verbose) fprintf(stderr, "[tap] CreateAggregateDevice err=%d\n", (int)err);
            destroy_all();
            return -13;
        }

        // The rate is read here, before any IOProc exists, so the DSP can be built for it.
        Float64 sr = 0; sz = sizeof(sr);
        get_prop(g_agg, kAudioDevicePropertyNominalSampleRate, &sz, &sr);
        if (sr <= 0) {
            sz = sizeof(sr);
            get_prop(out_dev, kAudioDevicePropertyNominalSampleRate, &sz, &sr);
        }
        if (sr <= 0) { destroy_all(); return -16; }
        if (verbose) fprintf(stderr, "[tap] created: %d procs, out=%s, sr=%.0f\n", n, out_uid.UTF8String, sr);
        *out_sr = sr;
        return 0;
    }
}

/// Installs the IOProc and starts the device. Requires a successful `ta_create`.
int ta_start(ta_cb cb, void *user) {
    if (g_agg == kAudioObjectUnknown) return -20;
    if (g_proc) return -21;
    OSStatus err = AudioDeviceCreateIOProcIDWithBlock(&g_proc, g_agg, NULL,
        ^(const AudioTimeStamp *__unused now, const AudioBufferList *in, const AudioTimeStamp *__unused inTime,
          AudioBufferList *out, const AudioTimeStamp *__unused outTime) {
            if (!in || in->mNumberBuffers == 0 || !out || out->mNumberBuffers == 0) return;
            const AudioBuffer *ib = &in->mBuffers[0];
            AudioBuffer *ob = &out->mBuffers[0];
            uint32_t in_ch = ib->mNumberChannels ? ib->mNumberChannels : 1;
            uint32_t out_ch = ob->mNumberChannels ? ob->mNumberChannels : 1;
            uint32_t frames = ib->mDataByteSize / (sizeof(float) * in_ch);
            uint32_t oframes = ob->mDataByteSize / (sizeof(float) * out_ch);
            if (oframes < frames) frames = oframes;
            cb((const float *)ib->mData, in_ch, (float *)ob->mData, out_ch, frames, user);
            for (UInt32 b = 1; b < out->mNumberBuffers; b++)
                memset(out->mBuffers[b].mData, 0, out->mBuffers[b].mDataByteSize);
        });
    if (err != noErr) { g_proc = NULL; return -14; }
    err = AudioDeviceStart(g_agg, g_proc);
    if (err != noErr) {
        AudioDeviceDestroyIOProcID(g_agg, g_proc);
        g_proc = NULL;
        return -15;
    }
    return 0;
}

void ta_stop(void) {
    destroy_all();
}
