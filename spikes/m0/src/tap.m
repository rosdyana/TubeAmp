// macOS Core Audio process tap shim (macOS 14.2+).
// Taps the audio of every process whose "responsible process" is `owner`
// (i.e. our own WebKit GPU/WebContent helpers), mutes the original output
// while tapped, and routes it through a private aggregate device whose main
// subdevice is the default output. The IOProc hands Rust the tapped input and
// the output buffers in one callback, so DSP is in-line with no extra queue.
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

/// Fills `out` with up to `cap` process object IDs owned by `owner`. Logs each process.
static int owned_processes(pid_t owner, AudioObjectID *out, int cap, int verbose) {
    AudioObjectPropertyAddress a = {kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 sz = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &a, 0, NULL, &sz) != noErr) return -1;
    int n = sz / sizeof(AudioObjectID);
    AudioObjectID *ids = malloc(sz);
    AudioObjectGetPropertyData(kAudioObjectSystemObject, &a, 0, NULL, &sz, ids);
    int found = 0;
    int owner_resp = responsibility_get_pid_responsible_for_pid(owner);
    for (int i = 0; i < n; i++) {
        pid_t pid = -1; UInt32 psz = sizeof(pid);
        get_prop(ids[i], kAudioProcessPropertyPID, &psz, &pid);
        UInt32 running = 0, rsz = sizeof(running);
        get_prop(ids[i], kAudioProcessPropertyIsRunningOutput, &rsz, &running);
        int resp = responsibility_get_pid_responsible_for_pid(pid);
        NSString *bid = get_str(ids[i], kAudioProcessPropertyBundleID);
        // Bundled app: helpers are responsible to us. Unbundled (cargo run from a
        // terminal): helpers share our responsible process, so also require a
        // WebKit bundle ID to avoid tapping unrelated terminal children.
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

int ta_count_owned(int owner) {
    AudioObjectID ids[16];
    return owned_processes(owner, ids, 16, 1);
}

int ta_start(int owner, ta_cb cb, void *user) {
    if (g_proc) return 0;
    @autoreleasepool {
        AudioObjectID ids[16];
        int n = owned_processes(owner, ids, 16, 1);
        if (n <= 0) return -10;
        NSMutableArray<NSNumber *> *procs = [NSMutableArray array];
        for (int i = 0; i < n; i++) [procs addObject:@(ids[i])];

        CATapDescription *desc = [[CATapDescription alloc] initStereoMixdownOfProcesses:procs];
        desc.name = @"TubeAmp tap";
        desc.privateTap = YES;
        desc.muteBehavior = CATapMutedWhenTapped;
        OSStatus err = AudioHardwareCreateProcessTap(desc, &g_tap);
        if (err != noErr) { fprintf(stderr, "[tap] CreateProcessTap err=%d\n", (int)err); return -11; }

        AudioObjectID out_dev = kAudioObjectUnknown; UInt32 sz = sizeof(out_dev);
        get_prop(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultOutputDevice, &sz, &out_dev);
        NSString *out_uid = get_str(out_dev, kAudioDevicePropertyDeviceUID);
        if (!out_uid) return -12;

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
        if (err != noErr) { fprintf(stderr, "[tap] CreateAggregateDevice err=%d\n", (int)err); return -13; }

        err = AudioDeviceCreateIOProcIDWithBlock(&g_proc, g_agg, NULL,
            ^(const AudioTimeStamp *now, const AudioBufferList *in, const AudioTimeStamp *inTime,
              AudioBufferList *out, const AudioTimeStamp *outTime) {
                if (!in || in->mNumberBuffers == 0 || !out || out->mNumberBuffers == 0) return;
                const AudioBuffer *ib = &in->mBuffers[0];
                AudioBuffer *ob = &out->mBuffers[0];
                uint32_t in_ch = ib->mNumberChannels ? ib->mNumberChannels : 1;
                uint32_t out_ch = ob->mNumberChannels ? ob->mNumberChannels : 1;
                uint32_t frames = ib->mDataByteSize / (sizeof(float) * in_ch);
                uint32_t oframes = ob->mDataByteSize / (sizeof(float) * out_ch);
                if (oframes < frames) frames = oframes;
                cb((const float *)ib->mData, in_ch, (float *)ob->mData, out_ch, frames, user);
                // Silence any extra output buffers (non-interleaved devices).
                for (UInt32 b = 1; b < out->mNumberBuffers; b++)
                    memset(out->mBuffers[b].mData, 0, out->mBuffers[b].mDataByteSize);
            });
        if (err != noErr) { fprintf(stderr, "[tap] CreateIOProc err=%d\n", (int)err); return -14; }
        err = AudioDeviceStart(g_agg, g_proc);
        if (err != noErr) { fprintf(stderr, "[tap] DeviceStart err=%d\n", (int)err); return -15; }

        Float64 sr = 0; sz = sizeof(sr);
        get_prop(g_agg, kAudioDevicePropertyNominalSampleRate, &sz, &sr);
        fprintf(stderr, "[tap] started: %d procs, out=%s, sr=%.0f\n", n, out_uid.UTF8String, sr);
        return (int)sr;
    }
}

void ta_stop(void) {
    if (g_proc) { AudioDeviceStop(g_agg, g_proc); AudioDeviceDestroyIOProcID(g_agg, g_proc); g_proc = NULL; }
    if (g_agg) { AudioHardwareDestroyAggregateDevice(g_agg); g_agg = kAudioObjectUnknown; }
    if (g_tap) { AudioHardwareDestroyProcessTap(g_tap); g_tap = kAudioObjectUnknown; }
}
