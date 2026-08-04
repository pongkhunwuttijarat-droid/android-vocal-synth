/*
 * plugin_abi.h — Unified voice-synthesis plugin ABI, version 1.
 *
 * This header defines the C contract between the host (OpenUtau-style
 * synthesizer host) and synthesis plugins. It is pure C (C99+), uses
 * C ABI-safe types only (no `bool`, no C++), and is the single source
 * of truth for the plugin interface.
 *
 * ------------------------------------------------------------------
 * ABI RULES (host and plugin MUST agree):
 *  1. Versioning: `abi_version` in PluginCapabilities MUST equal
 *     PLUGIN_ABI_VERSION. A host that sees a different abi_version
 *     MUST refuse to load the plugin.
 *  2. Ownership:
 *     - plugin_get_capabilities() returns a pointer to a static,
 *       plugin-owned struct. Never free it.
 *     - plugin_create() returns an opaque handle owned by the caller;
 *       it MUST be released with plugin_destroy().
 *     - plugin_render() allocates `out_samples` inside the plugin
 *       (malloc). The host MUST release it with plugin_free_samples()
 *       (never free() from the host side — allocators may differ).
 *     - All other buffers (RenderContext arrays) are host-owned and
 *       must stay valid for the duration of the plugin_render() call.
 *  3. Threading: a plugin handle must not be used concurrently from
 *     multiple threads. Distinct handles may be used from distinct
 *     threads. plugin_get_capabilities() must be thread-safe.
 *  4. Curves: each curve is a parallel array of `len` doubles, one
 *     value per `frame_size` ms (see RenderContext). A NULL pointer
 *     with len 0 means "not provided" — plugins must use defaults.
 *  5. Error codes: plugin_render() returns PLUGIN_OK (0) on success,
 *     a nonzero PLUGIN_ERR_* otherwise. On error, *out_samples is
 *     undefined and the plugin must not have leaked any allocation.
 *  6. Strings are NUL-terminated UTF-8, written into fixed-size
 *     buffers that include room for the terminator.
 * ------------------------------------------------------------------
 */

#ifndef PLUGIN_ABI_H_
#define PLUGIN_ABI_H_

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---- version ---------------------------------------------------- */

#define PLUGIN_ABI_VERSION 1u

/* ---- string/buffer sizes ---------------------------------------- */

#define PLUGIN_NAME_MAX       64    /* ids, short names              */
#define PLUGIN_STRING_MAX     128   /* versions, free-form strings   */
#define PLUGIN_PATH_MAX       1024  /* filesystem paths              */
#define PLUGIN_PHONEME_MAX    64    /* single phoneme symbol         */
#define PLUGIN_MAX_SINGER_TYPES 16  /* fixed array capacity          */
#define PLUGIN_MAX_EXPRESSIONS  16  /* fixed array capacity          */

/* ---- result codes ----------------------------------------------- */

#define PLUGIN_OK                  0
#define PLUGIN_ERR_UNSPECIFIED     1   /* generic failure             */
#define PLUGIN_ERR_BAD_HANDLE      2   /* NULL/invalid plugin handle  */
#define PLUGIN_ERR_BAD_CONTEXT     3   /* NULL/invalid RenderContext  */
#define PLUGIN_ERR_UNSUPPORTED     4   /* feature not supported       */
#define PLUGIN_ERR_ABI_MISMATCH    5   /* abi_version != PLUGIN_ABI_VERSION */
#define PLUGIN_ERR_BAD_SINGER      6   /* singer_path unusable        */
#define PLUGIN_ERR_ALLOC           7   /* out-of-memory               */

/* ---- expression descriptor -------------------------------------- */

typedef struct PluginExpression {
    char    id[PLUGIN_NAME_MAX];    /* stable expression id, e.g. "dynamics" */
    char    name[PLUGIN_STRING_MAX];/* human-readable name, e.g. "Dynamics" */
    float   default_value;          /* value when curve not provided        */
    float   min_value;              /* inclusive range                       */
    float   max_value;              /* inclusive range                       */
    uint8_t is_flag;                /* 1 = on/off flag, 0 = continuous      */
} PluginExpression;

/* ---- static plugin capabilities ---------------------------------- */
/*
 * Returned by plugin_get_capabilities(). Plugin-owned, never freed.
 * All counts are the number of VALID entries in the fixed arrays.
 */

typedef struct PluginCapabilities {
    /* identity */
    char    id[PLUGIN_NAME_MAX];         /* unique plugin id            */
    char    name[PLUGIN_NAME_MAX];       /* display name                */
    char    version[PLUGIN_STRING_MAX];  /* semantic version             */
    uint32_t abi_version;                /* MUST equal PLUGIN_ABI_VERSION */

    /* supported singer types (voicebanks), e.g. "utau", "vogen" */
    uint32_t singer_type_count;
    char     singer_types[PLUGIN_MAX_SINGER_TYPES][PLUGIN_NAME_MAX];

    /* supported expressions (curves) */
    uint32_t expression_count;
    PluginExpression expressions[PLUGIN_MAX_EXPRESSIONS];

    /* input requirements */
    uint8_t needs_wav_samples;      /* 1 = requires rendered wav data  */
    uint8_t needs_oto;              /* 1 = requires .oto aliases       */
    uint8_t needs_frq;              /* 1 = requires .frq pitch files   */
    uint8_t needs_vocoder;          /* 1 = requires WORLD vocoder      */
    uint8_t supports_curves;        /* 1 = accepts expression curves   */

    /* output format */
    uint32_t sample_rate;           /* preferred output rate; 0 = any  */
    uint8_t  channels;              /* 1 = mono, 2 = stereo            */

    /* prediction support */
    uint8_t has_pitch_predictor;    /* 1 = can predict note pitches    */
    uint8_t has_variance_predictor; /* 1 = can predict timing variance */
} PluginCapabilities;

/* ---- per-instance info ------------------------------------------- */
/*
 * Filled by plugin_create(). Host may pass NULL for `out_info` if the
 * instance details are not needed.
 */

typedef struct PluginInfo {
    char     id[PLUGIN_NAME_MAX];        /* mirrors capabilities.id      */
    char     version[PLUGIN_STRING_MAX]; /* resolved plugin version      */
    uint32_t sample_rate;                /* effective output rate        */
    uint8_t  channels;                   /* effective channel count      */
    uint32_t max_frame_size;             /* max samples per render call  */
} PluginInfo;

/* ---- one phoneme (note) inside a render request ------------------- */

typedef struct RenderPhoneme {
    char    phoneme[PLUGIN_PHONEME_MAX]; /* alias/symbol, e.g. "a"      */
    double  position_ms;   /* note start, relative to phrase start      */
    double  duration_ms;   /* full note duration                        */
    int32_t tone;          /* MIDI note number 0..127; -1 = rest        */
    double  leading_ms;    /* leading consonant duration (from oto)     */
    double  overlap_ms;    /* preutterance/overlap (from oto)           */
} RenderPhoneme;

/* ---- render request context --------------------------------------- */
/*
 * Everything the plugin needs to synthesize one phrase. Host-owned;
 * valid only for the duration of the plugin_render() call.
 */

typedef struct RenderContext {
    /* singer / voicebank */
    char singer_path[PLUGIN_PATH_MAX];  /* path to voicebank or singer  */

    /* note pitches (per phoneme, parallel to phonemes[]) */
    const int32_t* pitches;             /* MIDI pitches; may be NULL    */
    uint32_t       pitch_count;

    /* expression curves — one value per frame_size ms, frame-aligned
     * with the phrase timeline. NULL + len 0 = not provided. */
    const double* dynamics;    uint32_t dynamics_len;
    const double* gender;      uint32_t gender_len;
    const double* tension;     uint32_t tension_len;
    const double* breathiness; uint32_t breathiness_len;
    const double* voicing;     uint32_t voicing_len;

    /* phonemes in timeline order */
    const RenderPhoneme* phonemes;
    uint32_t             phoneme_count;

    /* timeline / format */
    uint32_t sample_rate;   /* synthesis sample rate (e.g. 44100)       */
    double   tempo;         /* beats per minute (e.g. 120.0)            */
    uint32_t frame_size;    /* ms per curve frame (0 = plugin default)  */
} RenderContext;

/* ---- export macro ------------------------------------------------- */

#if defined(_WIN32) || defined(__CYGWIN__)
#  if defined(PLUGIN_BUILDING_DLL)
#    define PLUGIN_API __declspec(dllexport)
#  elif defined(PLUGIN_IMPORTING_DLL)
#    define PLUGIN_API __declspec(dllimport)
#  else
#    define PLUGIN_API
#  endif
#elif defined(__GNUC__) && (__GNUC__ >= 4)
#  define PLUGIN_API __attribute__((visibility("default")))
#else
#  define PLUGIN_API
#endif

/* ---- function exports ---------------------------------------------- */

/* Static capabilities of this plugin. Never NULL; never freed. */
PLUGIN_API const PluginCapabilities* plugin_get_capabilities(void);

/*
 * Create a plugin instance. `config_path` may be NULL to use defaults.
 * On success returns an opaque handle (never NULL) and, if `out_info`
 * is non-NULL, fills it. On failure returns NULL and leaves
 * *out_info untouched.
 */
PLUGIN_API void* plugin_create(const char* config_path, PluginInfo* out_info);

/*
 * Synthesize one phrase. `ctx` must be non-NULL and fully populated.
 * On success (PLUGIN_OK) *out_samples points to plugin-allocated
 * interleaved float samples and *out_len is the sample count
 * (frames * channels). Host MUST call plugin_free_samples() on
 * *out_samples when done. On failure returns PLUGIN_ERR_* and leaves
 * *out_samples and *out_len untouched.
 */
PLUGIN_API int32_t plugin_render(void* handle, const RenderContext* ctx,
                                 float** out_samples, uint32_t* out_len);

/* Release samples allocated by plugin_render(). `samples` may be NULL. */
PLUGIN_API void plugin_free_samples(void* handle, float* samples);

/* Destroy an instance created by plugin_create(). `handle` may be NULL. */
PLUGIN_API void plugin_destroy(void* handle);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* PLUGIN_ABI_H_ */
