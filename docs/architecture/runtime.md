# Runtime Architecture

> Orchestration layer — manages jobs, parallelism, caching, mixing

---

## Modules

```
Runtime
├── Scheduler
│   ├─ Job queue
│   ├─ Cancel support
│   ├─ Progress tracking
│   └─ Retry logic
│
├── Chunker
│   ├─ Split phones into batches
│   ├─ Group by N-phones
│   └─ Handle dependencies
│
├── Cache
│   ├─ Hash-based lookup
│   ├─ LRU in-memory
│   └─ Disk persistence
│
├── Worker Pool
│   ├─ Parallel dispatch
│   ├─ Thread management
│   └─ Cancellation
│
└── Mixer
    ├─ Align by positionMs
    ├─ Sum audio
    └─ Apply track FX
```

---

## Scheduler

```kotlin
class Scheduler(
    private val engine: Engine,
    private val renderer: Renderer,
    private val chunker: Chunker,
    private val cache: CacheManager,
    private val pool: WorkerPool,
    private val mixer: Mixer
) {
    suspend fun renderJob(
        parts: List<RenderPart>,
        range: TickRange,
        onProgress: (Float) -> Unit
    ): FinalAudio {
        // 1. Engine: phonemize + feed
        val allPhones = engine.phonemize(parts)
        val phones = engine.feed.prepareInputs(allPhones)
        
        // 2. Chunker: split into batches
        val chunks = chunker.split(phones)
        
        // 3. Worker pool: parallel render
        val audios = pool.dispatch(chunks) { chunk ->
            cache.getOrCompute(chunk.hash) {
                renderer.render(chunk.phones)
            }
        }
        
        // 4. Mixer: combine
        onProgress(1f)
        return mixer.combine(audios, range)
    }
    
    fun cancelJob(jobId: String)
    fun getProgress(jobId: String): Float
}
```

---

## Chunker

```kotlin
interface Chunker {
    fun split(phones: List<PhoneRenderInput>): List<RenderChunk>
}

data class RenderChunk(
    val id: String,
    val phones: List<PhoneRenderInput>,
    val dependency: String? = null
)

// Implementation: split by N-phones per chunk
class SimpleChunker(private val chunkSize: Int = 8) : Chunker {
    override fun split(phones: List<PhoneRenderInput>): List<RenderChunk> {
        return phones.chunked(chunkSize).mapIndexed { index, chunk ->
            RenderChunk(
                id = "chunk-$index",
                phones = chunk
            )
        }
    }
}
```

---

## Cache

```kotlin
interface CacheManager {
    fun getOrCompute(hash: Long, compute: () -> AudioChunk): AudioChunk
    fun invalidate(hash: Long)
    fun clear()
}

// LRU in-memory cache
class LRUCache(private val capacity: Int) : CacheManager {
    private val cache = LinkedHashMap<Long, AudioChunk>(capacity, 0.75f, true)
    
    override fun getOrCompute(hash: Long, compute: () -> AudioChunk): AudioChunk {
        return cache.getOrPut(hash) {
            compute()
        }.also {
            if (cache.size > capacity) {
                cache.entries.firstOrNull()?.let { oldest ->
                    cache.remove(oldest.key)
                }
            }
        }
    }
}
```

---

## Worker Pool

```kotlin
interface WorkerPool {
    suspend fun dispatch(
        chunks: List<RenderChunk>,
        render: (RenderChunk) -> AudioChunk
    ): List<AudioChunk>
}

// Coroutine-based pool
class CoroutinePool(private val parallelism: Int = 4) : WorkerPool {
    override suspend fun dispatch(
        chunks: List<RenderChunk>,
        render: (RenderChunk) -> AudioChunk
    ): List<AudioChunk> = coroutineScope {
        chunks.map { chunk ->
            async(Dispatchers.Default) {
                render(chunk)
            }
        }.awaitAll()
    }
}
```

---

## Mixer

```kotlin
interface Mixer {
    fun combine(chunks: List<AudioChunk>, range: TickRange): FinalAudio
}

data class FinalAudio(
    val samples: FloatArray,    // mono or stereo
    val sampleRate: Int,        // 44100
    val durationMs: Double
)

class SimpleMixer : Mixer {
    override fun combine(chunks: List<AudioChunk>, range: TickRange): FinalAudio {
        // Calculate total length
        val totalSamples = calculateTotalSamples(chunks, range)
        val output = FloatArray(totalSamples)
        
        // Sum all chunks at their positions
        for (chunk in chunks) {
            val startSample = positionMsToSample(chunk.positionMs)
            for (i in chunk.samples.indices) {
                if (startSample + i < output.size) {
                    output[startSample + i] += chunk.samples[i]
                }
            }
        }
        
        return FinalAudio(
            samples = output,
            sampleRate = 44100,
            durationMs = range.durationMs
        )
    }
}
```

---

## Data Flow

```
Engine.phonemize(notes, singer)
    │
    ▼
Engine.feed.prepareInputs(phonemes)
    │
    ▼
PhoneRenderInput[]
    │
    ▼
Chunker.split(phones)
    │
    ▼
RenderChunk[]
    │
    ▼
WorkerPool.dispatch(chunks, renderer::render)
    │
    ▼
AudioChunk[] (parallel)
    │
    ▼
Mixer.combine(chunks, range)
    │
    ▼
FinalAudio
    │
    ▼
AudioService.play(audio)
```
