// Mango extension host.
//
// Runs Mihon-compatible extensions and exposes them to the Rust core over loopback
// HTTP. The contract is protocol/openapi.yaml. Suwayomi-Server's runtime classes
// (Android compatibility layer, source API, network stack, loaders) are used as a
// library from a stripped copy of its release jar; nothing of its server is started.

package mango.host

import android.os.Handler
import android.os.Looper
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.preference.EditTextPreference
import androidx.preference.ListPreference
import androidx.preference.MultiSelectListPreference
import androidx.preference.Preference
import androidx.preference.PreferenceScreen
import androidx.preference.TwoStatePreference
import ch.qos.logback.classic.Level
import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpServer
import eu.kanade.tachiyomi.App
import eu.kanade.tachiyomi.createAppModule
import eu.kanade.tachiyomi.network.NetworkHelper
import eu.kanade.tachiyomi.source.ConfigurableSource
import eu.kanade.tachiyomi.source.Source
import eu.kanade.tachiyomi.source.SourceFactory
import eu.kanade.tachiyomi.source.model.Filter
import eu.kanade.tachiyomi.source.model.FilterList
import eu.kanade.tachiyomi.source.model.MangasPage
import eu.kanade.tachiyomi.source.model.Page
import eu.kanade.tachiyomi.source.model.SChapter
import eu.kanade.tachiyomi.source.model.SManga
import eu.kanade.tachiyomi.source.model.UpdateStrategy
import eu.kanade.tachiyomi.source.online.HttpSource
import eu.kanade.tachiyomi.source.sourcePreferences
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import okhttp3.Request
import org.bouncycastle.jce.provider.BouncyCastleProvider
import org.cef.network.CefCookie
import org.cef.network.CefCookieManager
import org.koin.core.context.startKoin
import org.koin.dsl.module
import org.slf4j.Logger
import org.slf4j.LoggerFactory
import suwayomi.tachidesk.manga.impl.util.AndroidManifestParser
import suwayomi.tachidesk.manga.impl.util.PackageTools
import suwayomi.tachidesk.server.ApplicationDirs
import suwayomi.tachidesk.server.ServerConfig
import suwayomi.tachidesk.server.util.CEFManager
import suwayomi.tachidesk.server.util.ConfigTypeRegistration
import uy.kohesive.injekt.Injekt
import uy.kohesive.injekt.api.get
import xyz.nulldev.androidcompat.AndroidCompat
import xyz.nulldev.androidcompat.AndroidCompatInitializer
import xyz.nulldev.androidcompat.androidCompatModule
import xyz.nulldev.androidcompat.webkit.CefHelper
import xyz.nulldev.androidcompat.webkit.KcefWebViewProvider
import xyz.nulldev.ts.config.GlobalConfigManager
import xyz.nulldev.ts.config.configManagerModule
import java.io.File
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.security.Security
import java.time.Duration
import java.util.Date
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.zip.ZipFile
import kotlin.concurrent.thread
import kotlin.io.path.absolutePathString
import kotlin.system.exitProcess

private const val PROTOCOL = 5
private const val HOST_VERSION = "0.7.1"
private val PKG_NAME = Regex("[A-Za-z0-9_.]+")

private class ApiError(
    val status: Int,
    val code: String,
    message: String,
) : Exception(message)

private data class Installed(
    val pkg: String,
    val jar: String,
    val className: String,
    val name: String,
    val versionName: String,
    val versionCode: Int,
    val lib: String,
    val contentWarning: Int,
    val sources: JsonArray,
) {
    val sourceIds: List<Long> get() = sources.mapNotNull { it.jsonObject.str("id")?.toLongOrNull() }

    fun toJson(full: Boolean): JsonObject =
        buildJsonObject {
            put("pkg", pkg)
            if (full) {
                put("jar", jar)
                put("className", className)
            }
            put("name", name)
            put("versionName", versionName)
            put("versionCode", versionCode)
            put("lib", lib)
            put("contentWarning", contentWarning)
            put("sources", sources)
        }
}

private lateinit var extDir: Path
private var callTimeoutMs = 60_000L
private val installed = ConcurrentHashMap<String, Installed>()
private val sources = ConcurrentHashMap<Long, Source>()
private val loadedPkgs = ConcurrentHashMap.newKeySet<String>()
private val lastRequest = AtomicLong(System.currentTimeMillis())
private val downloader = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.ALWAYS).build()
private val webViewWanted = System.getenv("MANGO_HOST_WEBVIEW") == "1"
private val browserAsked = AtomicBoolean(false)
private lateinit var application: App

// ---------- JSON helpers ----------

private fun JsonObject.str(key: String): String? = (this[key] as? JsonPrimitive)?.contentOrNull

private fun JsonObject.int(
    key: String,
    default: Int,
): Int = (this[key] as? JsonPrimitive)?.intOrNull ?: default

private fun JsonObject.bool(
    key: String,
    default: Boolean,
): Boolean = (this[key] as? JsonPrimitive)?.booleanOrNull ?: default

private fun JsonObject.obj(key: String): JsonObject? = this[key] as? JsonObject

private fun bad(what: String): Nothing = throw ApiError(400, "bad_request", "Missing or invalid field: $what")

private fun mangaJson(m: SManga): JsonObject =
    buildJsonObject {
        put("url", runCatching { m.url }.getOrDefault(""))
        put("title", runCatching { m.title }.getOrDefault(""))
        put("artist", m.artist)
        put("author", m.author)
        put("description", m.description)
        put("genre", m.genre)
        put("status", m.status)
        put("thumbnailUrl", m.thumbnail_url)
        put("updateStrategy", m.update_strategy.name)
        put("initialized", m.initialized)
        put("memo", runCatching { m.memo }.getOrDefault(JsonObject(emptyMap())))
    }

private fun mangaFrom(o: JsonObject): SManga =
    SManga.create().apply {
        url = o.str("url") ?: bad("manga.url")
        title = o.str("title") ?: ""
        artist = o.str("artist")
        author = o.str("author")
        description = o.str("description")
        genre = o.str("genre")
        status = o.int("status", 0)
        thumbnail_url = o.str("thumbnailUrl")
        update_strategy = runCatching { UpdateStrategy.valueOf(o.str("updateStrategy")!!) }.getOrDefault(UpdateStrategy.ALWAYS_UPDATE)
        initialized = o.bool("initialized", false)
        o.obj("memo")?.let { memo = it }
    }

private fun chapterJson(c: SChapter): JsonObject =
    buildJsonObject {
        put("url", runCatching { c.url }.getOrDefault(""))
        put("name", runCatching { c.name }.getOrDefault(""))
        put("dateUpload", c.date_upload)
        put("chapterNumber", c.chapter_number)
        put("scanlator", c.scanlator)
        put("memo", runCatching { c.memo }.getOrDefault(JsonObject(emptyMap())))
    }

private fun chapterFrom(o: JsonObject): SChapter =
    SChapter.create().apply {
        url = o.str("url") ?: bad("chapter.url")
        name = o.str("name") ?: ""
        date_upload = (o["dateUpload"] as? JsonPrimitive)?.longOrNull ?: 0L
        chapter_number = (o["chapterNumber"] as? JsonPrimitive)?.floatOrNull ?: -1f
        scanlator = o.str("scanlator")
        o.obj("memo")?.let { memo = it }
    }

private fun pageJson(p: Page): JsonObject =
    buildJsonObject {
        put("index", p.index)
        put("url", p.url)
        put("imageUrl", p.imageUrl)
    }

private fun pageFrom(o: JsonObject): Page = Page(o.int("index", 0), o.str("url") ?: "", o.str("imageUrl"))

private fun mangasPageJson(page: MangasPage): JsonObject =
    buildJsonObject {
        put("mangas", JsonArray(page.mangas.map(::mangaJson)))
        put("hasNextPage", page.hasNextPage)
    }

private fun sourceJson(s: Source): JsonObject =
    buildJsonObject {
        put("id", s.id.toString())
        put("name", s.name)
        put("lang", s.lang)
        (s as? HttpSource)?.let { put("baseUrl", runCatching { it.baseUrl }.getOrDefault("")) }
        put("supportsLatest", s.supportsLatest)
    }

// ---------- runtime ----------

/** The part of Suwayomi's start-up that extensions depend on, and nothing else. */
private fun initRuntime(dataDir: Path) {
    (LoggerFactory.getLogger(Logger.ROOT_LOGGER_NAME) as ch.qos.logback.classic.Logger).level = Level.WARN
    System.setProperty("suwayomi.tachidesk.config.server.rootDir", dataDir.absolutePathString())

    // A source that meets a Cloudflare check hands it to a FlareSolverr, when the core
    // names one. The runtime's own network stack does the rest.
    val solver = System.getenv("MANGO_HOST_SOLVER_URL")?.trim().orEmpty()
    System.setProperty("suwayomi.tachidesk.config.server.flareSolverrEnabled", solver.isNotEmpty().toString())
    if (solver.isNotEmpty()) {
        System.setProperty("suwayomi.tachidesk.config.server.flareSolverrUrl", solver)
        System.setProperty("suwayomi.tachidesk.config.server.flareSolverrSessionName", "mango")
        System.getenv("MANGO_HOST_SOLVER_TIMEOUT_SECONDS")?.toIntOrNull()?.let {
            System.setProperty("suwayomi.tachidesk.config.server.flareSolverrTimeout", it.toString())
        }
    }

    // Android ships BouncyCastle's ciphers; some sources ask for ones the JVM lacks, such as AES/CBC/ZEROBYTEPADDING.
    Security.addProvider(BouncyCastleProvider())

    thread(name = "android-main-looper") {
        @Suppress("DEPRECATION")
        Looper.prepareMainLooper()
        Looper.loop()
    }

    System.setProperty("suwayomi.tachidesk.config.server.kcefEnabled", webViewWanted.toString())

    ConfigTypeRegistration.registerCustomTypes()
    GlobalConfigManager.registerModule(ServerConfig.register { GlobalConfigManager.config })

    val dirs = ApplicationDirs()
    listOf(dirs.dataRoot, dirs.extensionsRoot).forEach { File(it).mkdirs() }
    extDir = Path.of(dirs.extensionsRoot)

    val app = App()
    startKoin {
        modules(
            createAppModule(app),
            androidCompatModule(),
            configManagerModule(),
            module {
                single { dirs }
                single<KcefWebViewProvider.InitBrowserHandler> { CookieHandOver }
            },
        )
    }
    AndroidCompatInitializer().init()
    AndroidCompat().startApp(app)
    application = app
    prepareWebView()
}

/** A web view starts with the cookies the sources already hold. */
private object CookieHandOver : KcefWebViewProvider.InitBrowserHandler {
    override fun init(provider: KcefWebViewProvider) {
        val browser = CefCookieManager.getGlobalManager()
        for (cookie in Injekt.get<NetworkHelper>().cookieStore.getStoredCookies()) {
            runCatching {
                val kept = CefCookie(cookie.name, cookie.value, cookie.domain, cookie.path, cookie.secure, cookie.httpOnly, Date(), Date(), cookie.persistent, Date(cookie.expiresAt))
                browser.setCookie("https://" + cookie.domain, kept)
            }
        }
    }
}

/**
 * Some sources open a web view. Theirs is a Chromium the runtime downloads once and runs
 * without a window. It starts with the first web view, not with the host: most sources
 * never ask for one.
 */
private fun prepareWebView() {
    WebView.setProviderFactory { view ->
        if (browserAsked.compareAndSet(false, true)) {
            if (webViewWanted) {
                System.err.println("starting the browser for a web view; the first time it is downloaded, which takes a while")
                // A browser that was cut short leaves its lock behind, and the next one would refuse its profile.
                runCatching {
                    File(Injekt.get<ApplicationDirs>().cacheDir, "kcef").listFiles { file -> file.name.startsWith("Singleton") }?.forEach { it.delete() }
                }
                CEFManager.init()
            } else {
                CefHelper.cefApp.value = Result.failure(IllegalStateException("This source needs the web view, which is switched off"))
            }
        }
        KcefWebViewProvider(view)
    }
}

/** Opens a page in a web view and runs a script in it. Prints what came of it; for `check-webview`. */
private fun checkWebView(): Int {
    val answer = CompletableFuture<String>()
    Handler(Looper.getMainLooper()).post {
        runCatching {
            val view = WebView(application)
            view.settings.javaScriptEnabled = true
            view.webViewClient =
                object : WebViewClient() {
                    override fun onPageFinished(
                        view: WebView,
                        url: String,
                    ) {
                        view.evaluateJavascript("document.title + (6 * 7)") { answer.complete(it) }
                    }
                }
            view.loadUrl("data:text/html,<title>mango</title>")
        }.onFailure { answer.completeExceptionally(it) }
    }
    return try {
        // Long enough to download the browser on a slow line.
        println("MANGO_WEBVIEW_OK ${answer.get(20, TimeUnit.MINUTES)}")
        0
    } catch (e: Throwable) {
        val why = generateSequence(e) { it.cause }.last()
        println("MANGO_WEBVIEW_FAILED ${why.javaClass.simpleName}: ${why.message}")
        1
    }
}

private fun registryFile(): Path = extDir.resolve("installed.json")

private fun saveRegistry() {
    val json = buildJsonObject { installed.values.forEach { put(it.pkg, it.toJson(full = true)) } }
    Files.writeString(registryFile(), json.toString())
}

private fun loadRegistry() {
    val file = registryFile()
    if (!Files.exists(file)) return
    for ((pkg, value) in Json.parseToJsonElement(Files.readString(file)).jsonObject) {
        val o = value.jsonObject
        val jar = o.str("jar") ?: continue
        if (!Files.exists(Path.of(jar))) continue
        installed[pkg] =
            Installed(
                pkg = pkg,
                jar = jar,
                className = o.str("className") ?: continue,
                name = o.str("name") ?: pkg,
                versionName = o.str("versionName") ?: "",
                versionCode = o.int("versionCode", 0),
                lib = o.str("lib") ?: "",
                contentWarning = o.int("contentWarning", 0),
                sources = o["sources"] as? JsonArray ?: JsonArray(emptyList()),
            )
    }
}

/** Deletes the jars no installed extension owns: old versions that were still held when they were replaced. */
private fun sweepJars() {
    val owned = installed.values.map { Path.of(it.jar).fileName.toString() }.toSet()
    runCatching {
        Files.list(extDir).use { files ->
            files.filter { it.fileName.toString().endsWith(".jar") && it.fileName.toString() !in owned }.forEach { runCatching { Files.deleteIfExists(it) } }
        }
    }
}

private fun loadJar(
    jar: String,
    className: String,
): List<Source> =
    when (val instance = runBlocking { PackageTools.loadExtensionSources(Path.of(jar), className) }) {
        is Source -> listOf(instance)
        is SourceFactory -> instance.createSources()
        else -> throw ApiError(422, "bad_extension", "Unknown extension main class ${instance.javaClass.name}")
    }

private fun source(id: Long): HttpSource {
    (sources[id] as? HttpSource)?.let { return it }
    val ext =
        installed.values.firstOrNull { id in it.sourceIds }
            ?: throw ApiError(404, "unknown_source", "No installed extension provides source $id")
    synchronized(ext) {
        if (ext.pkg !in loadedPkgs) {
            loadJar(ext.jar, ext.className).forEach { sources[it.id] = it }
            loadedPkgs += ext.pkg
        }
    }
    return sources[id] as? HttpSource ?: throw ApiError(404, "unknown_source", "Source $id is gone from ${ext.pkg}")
}

private fun unload(pkg: String) {
    val ext = installed.remove(pkg) ?: return
    ext.sourceIds.forEach { sources.remove(it) }
    loadedPkgs -= pkg
    runCatching { PackageTools.jarLoaderMap.remove(Path.of(ext.jar).absolutePathString())?.close() }
    runCatching { Files.deleteIfExists(Path.of(ext.jar)) }
    saveRegistry()
}

private fun sha256(file: Path): String =
    MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(file)).joinToString("") { "%02x".format(it) }

@Synchronized
private fun install(body: JsonObject): JsonObject {
    val url = body.str("url") ?: bad("url")
    val expected = body.str("sha256")?.lowercase() ?: bad("sha256")
    if (!url.substringBefore('?').endsWith(".jar")) {
        throw ApiError(501, "apk_not_supported", "This host version installs .jar extensions only")
    }

    val tmp = Files.createTempFile(extDir, "download-", ".tmp")
    try {
        val response =
            downloader.send(
                HttpRequest.newBuilder(URI(url)).timeout(Duration.ofSeconds(120)).build(),
                HttpResponse.BodyHandlers.ofFile(tmp),
            )
        if (response.statusCode() != 200) throw ApiError(502, "download_failed", "HTTP ${response.statusCode()} for the extension file")
        if (sha256(tmp) != expected) throw ApiError(422, "sha256_mismatch", "Downloaded file does not match the expected sha256")

        val manifest =
            ZipFile(tmp.toFile()).use { zip ->
                val entry = zip.getEntry("AndroidManifest.xml") ?: throw ApiError(422, "bad_extension", "No AndroidManifest.xml in the jar")
                zip.getInputStream(entry).use { AndroidManifestParser.parse(it) }
            }
        val pkg = manifest.packageName
        if (!PKG_NAME.matches(pkg)) throw ApiError(422, "bad_extension", "Unsafe package name")
        val meta = manifest.application?.metaData.orEmpty()

        fun meta(name: String): String? = meta.firstOrNull { it.name == name }?.value?.trim()
        val declared = meta("tachiyomi.extension.class") ?: throw ApiError(422, "bad_extension", "No extension class declared")
        val className = if (declared.startsWith(".")) pkg + declared else declared
        val versionName = manifest.versionName ?: throw ApiError(422, "bad_extension", "No version name")

        // The core says which extension it asked the store for, and which version the store
        // lists. A file that is another extension would replace one it was never checked as;
        // an older version would bring back what an update had put right.
        val asked = body.str("pkg")
        if (asked != null && asked != pkg) throw ApiError(422, "bad_extension", "The file is $pkg, not the extension that was asked for")
        val listed = body.str("versionCode")?.toLongOrNull()
        if (listed != null && (manifest.versionCode ?: 0) < listed) {
            throw ApiError(422, "bad_extension", "The file is an older version than the store lists")
        }

        unload(pkg)
        // A file name of its own each time. Where a jar that was loaded cannot be replaced
        // or deleted yet, the old file stays behind until the host starts again.
        val jar = extDir.resolve("$pkg-v$versionName-${System.currentTimeMillis().toString(36)}.jar")
        Files.move(tmp, jar, StandardCopyOption.REPLACE_EXISTING)

        val loaded =
            try {
                loadJar(jar.absolutePathString(), className)
            } catch (e: Throwable) {
                runCatching { Files.deleteIfExists(jar) }
                throw e
            }
        loaded.forEach { sources[it.id] = it }
        loadedPkgs += pkg

        val ext =
            Installed(
                pkg = pkg,
                jar = jar.absolutePathString(),
                className = className,
                name = meta("tachiyomix.name") ?: manifest.application?.label?.substringAfter("Tachiyomi: ") ?: pkg,
                versionName = versionName,
                versionCode = manifest.versionCode ?: 0,
                lib = meta("tachiyomix.extensionLib")?.takeUnless { it == "0" } ?: versionName.substringBeforeLast('.'),
                contentWarning = meta("tachiyomix.contentWarning")?.toIntOrNull() ?: meta("tachiyomi.extension.nsfw")?.toIntOrNull() ?: 0,
                sources = JsonArray(loaded.map(::sourceJson)),
            )
        installed[pkg] = ext
        saveRegistry()
        return ext.toJson(full = false)
    } finally {
        Files.deleteIfExists(tmp)
    }
}

/** Runs a call into extension code with a time limit and maps its failures. */
private fun <T> call(block: suspend () -> T): T =
    try {
        runBlocking { withTimeout(callTimeoutMs) { block() } }
    } catch (e: ApiError) {
        throw e
    } catch (e: TimeoutCancellationException) {
        throw ApiError(504, "timeout", "The source did not answer in time")
    } catch (e: LinkageError) {
        throw ApiError(501, "needs_optional_module", "This source needs a module that is not installed: ${e.message}")
    } catch (e: Throwable) {
        throw ApiError(502, "source_error", e.message ?: e.javaClass.simpleName)
    }

// ---------- preferences ----------

/** What a source lets its user set, drawn the way it would draw it on a phone. */
private fun drawScreen(src: Source): PreferenceScreen =
    call {
        val screen = PreferenceScreen(application)
        if (src is ConfigurableSource) {
            screen.sharedPreferences = src.sourcePreferences()
            src.setupPreferenceScreen(screen)
        }
        screen
    }

private fun words(list: Array<CharSequence>?): JsonArray = JsonArray(list.orEmpty().map { JsonPrimitive(it.toString()) })

private fun preferenceJson(
    position: Int,
    p: Preference,
): JsonObject =
    buildJsonObject {
        val value = runCatching { p.currentValue }.getOrNull()
        put("position", position)
        put("key", p.key)
        put("title", p.title?.toString())
        put("summary", runCatching { p.summary?.toString() }.getOrNull())
        put("enabled", p.isEnabled)
        put("visible", p.visible)
        when (p) {
            is TwoStatePreference -> {
                put("type", "switch")
                put("value", value as? Boolean ?: false)
            }

            is ListPreference -> {
                put("type", "list")
                put("value", value as? String)
                put("entries", words(p.entries))
                put("entryValues", words(p.entryValues))
            }

            is MultiSelectListPreference -> {
                put("type", "multi")
                put("value", JsonArray((value as? Set<*>).orEmpty().map { JsonPrimitive(it.toString()) }))
                put("entries", words(p.entries))
                put("entryValues", words(p.entryValues))
            }

            is EditTextPreference -> {
                put("type", "text")
                put("value", value as? String ?: "")
                put("dialogTitle", p.dialogTitle?.toString())
                put("dialogMessage", p.dialogMessage?.toString())
            }

            else -> {
                put("type", "info")
            }
        }
    }

private fun preferencesJson(src: Source): JsonObject =
    buildJsonObject {
        put("preferences", JsonArray(drawScreen(src).preferences.mapIndexed(::preferenceJson)))
    }

private fun setPreference(
    src: Source,
    body: JsonObject,
) {
    val pref = drawScreen(src).preferences.getOrNull(body.int("position", -1)) ?: bad("position")
    // The screen is drawn anew for each call, and a source may draw it differently the second time.
    if (pref.key != body.str("key")) bad("key: the preferences changed, read them again")
    if (!pref.isEnabled) bad("position: this preference is switched off")
    val given = body["value"]
    val value: Any =
        when (pref) {
            is TwoStatePreference -> (given as? JsonPrimitive)?.booleanOrNull
            is ListPreference, is EditTextPreference -> (given as? JsonPrimitive)?.takeIf { it.isString }?.content
            is MultiSelectListPreference -> (given as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }?.toSet()
            else -> bad("position: nothing to set there")
        } ?: bad("value")
    call {
        // As on a phone: the source hears of the change first and may refuse it.
        val accepted = runCatching { pref.callChangeListener(value) }.getOrDefault(true)
        if (accepted && pref.key != null) pref.saveNewValue(value)
    }
    // Sources read their preferences when they are made: the extension's are made anew at the next call.
    val ext = installed.values.firstOrNull { src.id in it.sourceIds } ?: return
    synchronized(ext) {
        ext.sourceIds.forEach { sources.remove(it) }
        loadedPkgs -= ext.pkg
    }
}

// ---------- filters ----------

/** One of the things a source's search can be narrowed by. Its place in the list names it. */
private fun filterJson(f: Filter<*>): JsonObject =
    buildJsonObject {
        put("name", f.name)
        when (f) {
            is Filter.Select<*> -> {
                put("type", "select")
                put("state", f.state)
                put("values", JsonArray(f.values.map { JsonPrimitive(it.toString()) }))
            }

            is Filter.Text -> {
                put("type", "text")
                put("state", f.state)
            }

            is Filter.CheckBox -> {
                put("type", "checkbox")
                put("state", f.state)
            }

            is Filter.TriState -> {
                put("type", "tristate")
                put("state", f.state)
            }

            is Filter.Sort -> {
                put("type", "sort")
                put("values", JsonArray(f.values.map { JsonPrimitive(it) }))
                val picked = f.state
                put(
                    "state",
                    if (picked == null) {
                        JsonNull
                    } else {
                        buildJsonObject {
                            put("index", picked.index)
                            put("ascending", picked.ascending)
                        }
                    },
                )
            }

            is Filter.Group<*> -> {
                put("type", "group")
                put("filters", JsonArray(f.state.map { inner -> (inner as? Filter<*>)?.let(::filterJson) ?: headerJson(inner.toString()) }))
            }

            is Filter.Separator -> {
                put("type", "separator")
            }

            else -> {
                put("type", "header")
            }
        }
    }

private fun headerJson(name: String): JsonObject =
    buildJsonObject {
        put("name", name)
        put("type", "header")
    }

/**
 * Sets the filters a search names, `[{position, state}]`, on a list the source has just made.
 * A group's state is such a list again, for the filters inside it.
 */
private fun applyFilters(
    list: List<*>,
    changes: List<JsonElement>,
) {
    for (change in changes) {
        val o = change as? JsonObject ?: bad("filters")
        val at = o.int("position", -1)
        val filter = list.getOrNull(at) as? Filter<*> ?: bad("filters: nothing at $at, read the filters again")
        val given = o["state"]
        val plain = given as? JsonPrimitive

        fun wrong(): Nothing = bad("filters: the state for \"${filter.name}\" at $at is not one it takes, read the filters again")
        when (filter) {
            is Filter.Select<*> -> {
                filter.state = plain?.intOrNull?.takeIf { it in filter.values.indices } ?: wrong()
            }

            is Filter.Text -> {
                filter.state = plain?.takeIf { it.isString }?.content ?: wrong()
            }

            is Filter.CheckBox -> {
                filter.state = plain?.takeUnless { it.isString }?.booleanOrNull ?: wrong()
            }

            is Filter.TriState -> {
                filter.state = plain?.intOrNull?.takeIf { it in 0..2 } ?: wrong()
            }

            is Filter.Sort -> {
                filter.state =
                    if (given == null || given is JsonNull) {
                        null
                    } else {
                        val picked = given as? JsonObject ?: wrong()
                        val index = picked.int("index", -1).takeIf { it in filter.values.indices } ?: wrong()
                        Filter.Sort.Selection(index, picked.bool("ascending", false))
                    }
            }

            is Filter.Group<*> -> {
                applyFilters(filter.state, given as? JsonArray ?: wrong())
            }

            else -> {
                wrong()
            }
        }
    }
}

private fun rssMb(): Long =
    try {
        Files
            .readAllLines(Path.of("/proc/self/status"))
            .first { it.startsWith("VmRSS") }
            .filter { it.isDigit() }
            .toLong() / 1024
    } catch (_: Exception) {
        -1
    }

// ---------- HTTP ----------

private fun readBody(ex: HttpExchange): JsonObject {
    val text = ex.requestBody.use { it.readBytes().decodeToString() }
    if (text.isBlank()) return JsonObject(emptyMap())
    return runCatching { Json.parseToJsonElement(text).jsonObject }.getOrElse { bad("body is not a JSON object") }
}

private fun send(
    ex: HttpExchange,
    status: Int,
    body: JsonElement,
) {
    val bytes = body.toString().encodeToByteArray()
    ex.responseHeaders.add("Content-Type", "application/json; charset=utf-8")
    ex.sendResponseHeaders(status, bytes.size.toLong())
    ex.responseBody.use { it.write(bytes) }
}

private fun route(ex: HttpExchange) {
    val method = ex.requestMethod
    val parts = ex.requestURI.path.trim('/').split('/')
    val path = parts.joinToString("/")

    when {
        method == "GET" && path == "health" -> {
            send(
                ex,
                200,
                buildJsonObject {
                    put("protocol", PROTOCOL)
                    put("host", HOST_VERSION)
                    put("runtime", System.getenv("MANGO_RUNTIME_VERSION") ?: "unknown")
                    put("installedExtensions", installed.size)
                    put("loadedExtensions", loadedPkgs.size)
                    put("loadedSources", sources.size)
                    put("rssMb", rssMb())
                },
            )
        }

        method == "GET" && path == "extensions" -> {
            send(ex, 200, buildJsonObject { put("extensions", buildJsonArray { installed.values.sortedBy { it.pkg }.forEach { add(it.toJson(false)) } }) })
        }

        method == "POST" && path == "extensions/install" -> {
            send(ex, 200, install(readBody(ex)))
        }

        method == "DELETE" && parts.size == 2 && parts[0] == "extensions" -> {
            unload(parts[1])
            send(ex, 200, buildJsonObject { put("removed", true) })
        }

        parts.size == 3 && parts[0] == "sources" -> {
            val id = parts[1].toLongOrNull() ?: throw ApiError(404, "unknown_source", "Bad source id")
            routeSource(ex, method, id, parts[2])
        }

        else -> {
            throw ApiError(404, "not_found", "No such endpoint")
        }
    }
}

private fun routeSource(
    ex: HttpExchange,
    method: String,
    id: Long,
    action: String,
) {
    if (action == "filters" && method == "GET") {
        val src = source(id)
        send(ex, 200, buildJsonObject { put("filters", JsonArray(call { src.getFilterList() }.map(::filterJson))) })
        return
    }
    if (action == "preferences" && (method == "GET" || method == "PUT")) {
        if (method == "PUT") setPreference(source(id), readBody(ex))
        send(ex, 200, preferencesJson(source(id)))
        return
    }
    if (method != "POST") throw ApiError(404, "not_found", "No such endpoint")

    val src = source(id)
    val body = readBody(ex)
    val page = body.int("page", 1)

    when (action) {
        "popular" -> {
            send(ex, 200, mangasPageJson(call { src.getPopularManga(page) }))
        }

        "latest" -> {
            send(ex, 200, mangasPageJson(call { src.getLatestUpdates(page) }))
        }

        "search" -> {
            val changes = body["filters"]?.let { it as? JsonArray ?: bad("filters") }.orEmpty()
            val found =
                call {
                    // As on a phone: a search carries the source's own list, changed or not.
                    // A source that cannot make its list is searched without one, as before.
                    val filters = runCatching { src.getFilterList() }.getOrElse { if (changes.isEmpty()) FilterList() else throw it }
                    applyFilters(filters, changes)
                    src.getSearchManga(page, body.str("query") ?: "", filters)
                }
            send(ex, 200, mangasPageJson(found))
        }

        "manga" -> {
            val manga = mangaFrom(body.obj("manga") ?: bad("manga"))
            val known = (body["chapters"] as? JsonArray).orEmpty().map { chapterFrom(it.jsonObject) }
            val update = call { src.getMangaUpdate(manga, known, body.bool("fetchDetails", true), body.bool("fetchChapters", true)) }
            send(
                ex,
                200,
                buildJsonObject {
                    put("manga", mangaJson(update.manga))
                    put("chapters", JsonArray(update.chapters.map(::chapterJson)))
                },
            )
        }

        "pages" -> {
            val chapter = chapterFrom(body.obj("chapter") ?: bad("chapter"))
            send(ex, 200, buildJsonObject { put("pages", JsonArray(call { src.getPageList(chapter) }.map(::pageJson))) })
        }

        "image" -> {
            val cover = body.str("cover")
            val response =
                if (cover != null) {
                    // A cover is not a page: sources that build page requests their own
                    // way cannot make one from it. It is fetched the plain way, with
                    // the source's client and headers.
                    val address = if (cover.startsWith("/")) src.baseUrl.trimEnd('/') + cover else cover
                    call {
                        val answer = src.client.newCall(Request.Builder().url(address).headers(src.headers).build()).execute()
                        if (!answer.isSuccessful) {
                            answer.close()
                            throw ApiError(502, "source_error", "HTTP ${answer.code} for the cover")
                        }
                        answer
                    }
                } else {
                    val target = pageFrom(body.obj("page") ?: bad("page or cover"))
                    call {
                        if (target.imageUrl.isNullOrEmpty()) target.imageUrl = src.getImageUrl(target)
                        src.getImage(target)
                    }
                }
            response.use { r ->
                ex.responseHeaders.add("Content-Type", r.header("Content-Type") ?: "application/octet-stream")
                ex.sendResponseHeaders(200, 0)
                ex.responseBody.use { out -> r.body.byteStream().copyTo(out) }
            }
        }

        else -> {
            throw ApiError(404, "not_found", "No such endpoint")
        }
    }
}

private fun handle(
    ex: HttpExchange,
    token: ByteArray,
) {
    lastRequest.set(System.currentTimeMillis())
    ex.responseHeaders.add("X-Mango-Protocol", PROTOCOL.toString())
    try {
        val given = ex.requestHeaders.getFirst("Authorization")?.removePrefix("Bearer ")?.encodeToByteArray() ?: ByteArray(0)
        if (!MessageDigest.isEqual(given, token)) throw ApiError(401, "unauthorized", "Missing or wrong token")
        route(ex)
    } catch (e: Throwable) {
        val error = e as? ApiError ?: ApiError(500, "internal", e.message ?: e.javaClass.simpleName)
        runCatching {
            send(
                ex,
                error.status,
                buildJsonObject {
                    put(
                        "error",
                        buildJsonObject {
                            put("code", error.code)
                            put("message", error.message ?: "")
                        },
                    )
                },
            )
        }
    } finally {
        ex.close()
    }
}

fun main(args: Array<String>) {
    val dataDir = Path.of(System.getenv("MANGO_HOST_DATA") ?: "data").toAbsolutePath()
    // `check-webview` tries the web view and nothing else: no token, no port.
    if (args.firstOrNull() == "check-webview") {
        Files.createDirectories(dataDir)
        initRuntime(dataDir)
        exitProcess(checkWebView())
    }
    val token = System.getenv("MANGO_HOST_TOKEN")?.takeIf { it.length >= 16 }
    if (token == null) {
        System.err.println("MANGO_HOST_TOKEN must be set to at least 16 characters")
        exitProcess(2)
    }
    // The core starts the host with a pipe on stdin and keeps the other end. When that
    // pipe closes the core has let go or died, and the host must not outlive it. A
    // kill alone does not always arrive: `java` can be a launcher that runs the JVM
    // as a child of its own.
    if (System.getenv("MANGO_HOST_WATCH_STDIN") == "1") {
        thread(isDaemon = true, name = "mango-stdin") {
            runCatching { while (System.`in`.read() != -1) Unit }
            exitProcess(0)
        }
    }
    val port = System.getenv("MANGO_HOST_PORT")?.toIntOrNull() ?: 0
    val idleSeconds = System.getenv("MANGO_HOST_IDLE_SECONDS")?.toLongOrNull() ?: 0
    System.getenv("MANGO_HOST_CALL_TIMEOUT_SECONDS")?.toLongOrNull()?.let { callTimeoutMs = it * 1000 }

    Files.createDirectories(dataDir)
    initRuntime(dataDir)
    loadRegistry()
    sweepJars()

    val server = HttpServer.create(InetSocketAddress(InetAddress.getLoopbackAddress(), port), 64)
    server.executor = Executors.newCachedThreadPool { r -> Thread(r, "mango-http").apply { isDaemon = true } }
    val tokenBytes = token.encodeToByteArray()
    server.createContext("/") { ex -> handle(ex, tokenBytes) }
    server.start()

    if (idleSeconds > 0) {
        thread(isDaemon = true, name = "mango-idle") {
            while (true) {
                Thread.sleep(1000)
                if (System.currentTimeMillis() - lastRequest.get() > idleSeconds * 1000) exitProcess(0)
            }
        }
    }

    // The core waits for this line.
    println("MANGO_HOST_READY port=${server.address.port} protocol=$PROTOCOL")
}
