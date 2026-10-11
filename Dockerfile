# syntax=docker/dockerfile:1
# Mango: the reader and its extension host.
#
#   docker build -t mango .                        the image
#   docker build --target full -t mango:full .     the image with what a web view needs
#   docker buildx build --target dist -o out .     the binary and the host's jars, to run without a container
#   docker buildx build --target jars -o out .     the host's two jars alone
#   docker build --target host -t mango-host .     the extension host on its own
#   docker buildx build --target shell -o out .    the desktop shell for Linux
#   docker buildx build --target notices -o out .  the licences of what Mango is built from
#
# The host is two jars:
#   mango-host.jar    our code (mango-extension-host/src)
#   runtime.jar       Suwayomi-Server's release jar with the server parts removed
# The Suwayomi jar is MPL-2.0; its source is at
# https://github.com/Suwayomi/Suwayomi-Server at the tag named below.

ARG SUWAYOMI_VERSION=v2.4.2366
ARG SUWAYOMI_SHA256=af9feb20af9d7ebe9e30769e6c6ebc7fc7ab1c447388d42e0280b1da5fe07bfd
ARG KOTLIN_VERSION=2.4.21
ARG KOTLIN_SHA256=7cc140e76daf416a0424a5557f99afd7ca89ebd463ea1101261e3e01c33e75cd
ARG CARGO_ABOUT_VERSION=0.9.2

# ---------- the extension host's jars ----------
# Java bytecode: built on the machine doing the build, whatever the image is for.

FROM --platform=$BUILDPLATFORM eclipse-temurin:21-jdk AS host-build
ARG SUWAYOMI_VERSION
ARG SUWAYOMI_SHA256
ARG KOTLIN_VERSION
ARG KOTLIN_SHA256
RUN apt-get update && apt-get install -y --no-install-recommends curl unzip zip && rm -rf /var/lib/apt/lists/*
WORKDIR /build
RUN curl -fsSL -o suwayomi.jar \
      "https://github.com/Suwayomi/Suwayomi-Server/releases/download/${SUWAYOMI_VERSION}/Suwayomi-Server-${SUWAYOMI_VERSION}.jar" \
 && echo "${SUWAYOMI_SHA256}  suwayomi.jar" | sha256sum -c -
RUN curl -fsSL -o kotlinc.zip \
      "https://github.com/JetBrains/kotlin/releases/download/v${KOTLIN_VERSION}/kotlin-compiler-${KOTLIN_VERSION}.zip" \
 && echo "${KOTLIN_SHA256}  kotlinc.zip" | sha256sum -c - \
 && unzip -q kotlinc.zip && rm kotlinc.zip

COPY mango-extension-host/src src
RUN mkdir out && kotlinc/bin/kotlinc src -cp suwayomi.jar -d out/mango-host.jar -jvm-target 21 -nowarn

# Remove what the host never touches: JS engine, web server, GraphQL, databases,
# web UI, tray, Android framework resources. The classes that drive the browser
# for web views stay; the browser itself is not in the jar.
RUN cp suwayomi.jar out/runtime.jar \
 && (zip -q -d out/runtime.jar \
      'com/oracle/*' 'org/graalvm/*' 'META-INF/native-image/*' \
      'org/eclipse/jetty/*' 'jakarta/servlet/*' 'io/javalin/*' 'gg/jte/*' 'tools/jackson/*' \
      'graphql/*' 'com/expediagroup/*' 'org/dataloader/*' \
      'org/h2/*' 'org/postgresql/*' 'com/zaxxer/*' \
      'dorkbox/*' 'WebUI.zip' 'resources.arsc' 'res/*' \
    || [ $? -eq 12 ]) \
 && ls -l out

FROM scratch AS jars
COPY --from=host-build /build/out /

# The host with nothing around it, for its own smoke test.
FROM eclipse-temurin:21-jre AS host
ARG SUWAYOMI_VERSION
RUN useradd --system --uid 10001 --home-dir /data mango && mkdir /data && chown mango /data
COPY --from=host-build /build/out /opt/mango-host
USER mango
ENV HOME=/data \
    MANGO_HOST_DATA=/data \
    MANGO_RUNTIME_VERSION=${SUWAYOMI_VERSION}
ENTRYPOINT ["java", "-XX:+UseSerialGC", "-Xmx256m", "-Xss512k", "-XX:TieredStopAtLevel=1", "-Djava.awt.headless=true", \
            "-cp", "/opt/mango-host/mango-host.jar:/opt/mango-host/runtime.jar", "mango.host.MainKt"]

# ---------- the web UI ----------

FROM --platform=$BUILDPLATFORM node:22-slim AS ui
WORKDIR /ui
COPY mango-reader/ui/package.json mango-reader/ui/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY mango-reader/ui ./
RUN npm run build

# ---------- the core ----------
# Compiled on the machine doing the build, for the machine the image is for.

FROM --platform=$BUILDPLATFORM rust:1-bookworm AS core
ARG TARGETARCH
RUN case "$TARGETARCH" in \
      amd64) echo x86_64-unknown-linux-gnu x86_64-linux-gnu ;; \
      arm64) echo aarch64-unknown-linux-gnu aarch64-linux-gnu ;; \
      *) echo "Mango is not built for $TARGETARCH" >&2; exit 1 ;; \
    esac > /target \
 && read -r triple gnu < /target \
 && rustup target add "$triple" \
 && if [ "$(uname -m)-linux-gnu" != "$gnu" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends "gcc-$(echo "$gnu" | tr _ -)" "libc6-dev-$TARGETARCH-cross" \
      && rm -rf /var/lib/apt/lists/*; \
    fi
WORKDIR /src
COPY mango-reader/Cargo.toml mango-reader/Cargo.lock mango-reader/build.rs ./
COPY mango-reader/migrations migrations
COPY mango-reader/src src
COPY --from=ui /ui/dist ui/dist
# A release build says which version it is; any other build goes by Cargo.toml.
ARG MANGO_VERSION=
ARG MANGO_COMMIT=
ARG MANGO_REPO=
# Each platform keeps its own caches: built side by side, two would unpack the
# same crates into one registry at once, and wait on each other's build folder.
RUN --mount=type=cache,id=mango-registry-$TARGETARCH,target=/usr/local/cargo/registry \
    --mount=type=cache,id=mango-target-$TARGETARCH,target=/src/target \
    read -r triple gnu < /target \
 && export "CARGO_TARGET_$(echo "$triple" | tr 'a-z-' 'A-Z_')_LINKER=$gnu-gcc" "CC_$(echo "$triple" | tr - _)=$gnu-gcc" \
 && cargo build --release --locked --target "$triple" \
 && cp "target/$triple/release/mango" /mango

# ---------- other people's licences ----------
# One text file: the licence of every crate in the server and in the desktop shell,
# then those of what the web UI carries. Only the manifests are read, so the two
# programs' sources are stood in for by empty files.

FROM --platform=$BUILDPLATFORM rust:1-bookworm AS notices-build
ARG CARGO_ABOUT_VERSION
RUN cargo install cargo-about --locked --version "$CARGO_ABOUT_VERSION" --features cli && rm -rf /usr/local/cargo/registry
WORKDIR /src
COPY mango-reader/Cargo.toml mango-reader/Cargo.lock mango-reader/build.rs mango-reader/about.toml mango-reader/about.hbs mango-reader/
COPY mango-desktop/Cargo.toml mango-desktop/Cargo.lock mango-desktop/
COPY --from=ui /ui/node_modules/svelte/LICENSE.md ui/svelte
COPY --from=ui /ui/node_modules/@fontsource/dela-gothic-one/LICENSE ui/dela-gothic-one
COPY --from=ui /ui/node_modules/@fontsource/zen-kaku-gothic-new/LICENSE ui/zen-kaku-gothic-new
RUN --mount=type=cache,id=mango-notices-registry,target=/usr/local/cargo/registry \
    mkdir mango-reader/src mango-desktop/src && touch mango-reader/src/main.rs mango-desktop/src/main.rs \
 && rule=-------------------------------------------------------------------------------- \
 && { echo "Mango is built from the work of others. Their licences follow: first the Rust" \
   && echo "crates in the server, then those in the desktop shell, then what the web UI carries." \
   && for part in "mango, the server:mango-reader" "mango-desktop, the desktop shell:mango-desktop"; do \
        printf '\n\n%s\n%s\n%s\n\n' "$rule" "${part%%:*}" "$rule" \
        && cargo about generate --locked -m "${part#*:}/Cargo.toml" -c mango-reader/about.toml mango-reader/about.hbs \
        || exit 1; \
      done \
   && for part in "Svelte:svelte" "Dela Gothic One:dela-gothic-one" "Zen Kaku Gothic New:zen-kaku-gothic-new"; do \
        printf '\n\n%s\n%s, in the web UI\n%s\n\n' "$rule" "${part%%:*}" "$rule" && cat "ui/${part#*:}" || exit 1; \
      done; } > /THIRD-PARTY-LICENSES.txt \
 && test "$(wc -c < /THIRD-PARTY-LICENSES.txt)" -gt 20000

FROM scratch AS notices
COPY --from=notices-build /THIRD-PARTY-LICENSES.txt /

# What a release archive holds: the binary, the host beside it, and the licences.
FROM scratch AS dist
COPY --from=core /mango /mango
COPY --from=host-build /build/out /host
COPY --from=notices-build /THIRD-PARTY-LICENSES.txt /

# ---------- the desktop shell ----------
# A window and a tray icon around the binary. It links the system's GTK and WebKit,
# so it is built for the machine doing the build only, on the same Debian as the core.

FROM rust:1-bookworm AS shell-build
RUN apt-get update \
 && apt-get install -y --no-install-recommends libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev libxdo-dev \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY mango-desktop/Cargo.toml mango-desktop/Cargo.lock ./
COPY mango-desktop/src src
RUN --mount=type=cache,id=mango-shell-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=mango-shell-target,target=/src/target \
    cargo build --release --locked \
 && cp target/release/mango-desktop /mango-desktop

FROM scratch AS shell
COPY --from=shell-build /mango-desktop /mango-desktop

# ---------- a Java runtime with only what the host uses ----------
# Linked on the machine doing the build, from the target's modules. The first stage
# is there for those modules alone; nothing is run from it.

FROM eclipse-temurin:21-jdk AS target-jdk

FROM --platform=$BUILDPLATFORM eclipse-temurin:21-jdk AS java
ARG BUILDARCH
ARG TARGETARCH
COPY --from=target-jdk /opt/java/openjdk/jmods /jmods
# The class-data archive makes the host start faster. Making it means running the
# linked runtime, which only works when it is for this machine. Only Java's debug
# data is stripped: the native libraries come stripped, and this machine's tools
# could not strip another's anyway.
RUN jlink --module-path /jmods --output /java \
      --strip-java-debug-attributes --no-man-pages --no-header-files \
      $([ "$BUILDARCH" = "$TARGETARCH" ] && echo --generate-cds-archive) \
      --add-modules java.base,java.compiler,java.desktop,java.instrument,java.logging,java.management,java.naming,java.net.http,java.prefs,java.rmi,java.scripting,java.security.jgss,java.security.sasl,java.sql,java.xml,jdk.charsets,jdk.crypto.cryptoki,jdk.crypto.ec,jdk.httpserver,jdk.localedata,jdk.management,jdk.naming.dns,jdk.net,jdk.unsupported,jdk.xml.dom,jdk.zipfs

# ---------- the images ----------
# `mango`, the last stage, is what a plain build gives. `full` is the same with what
# a web view needs.

FROM debian:bookworm-slim AS base
ARG SUWAYOMI_VERSION
# Fonts are for the sources that draw text onto their pages.
RUN apt-get update \
 && apt-get install -y --no-install-recommends fontconfig fonts-dejavu-core libfreetype6 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data mango && mkdir /data && chown mango /data
COPY --from=java /java /opt/java
COPY --from=host-build /build/out /opt/mango/host
COPY LICENSE NOTICE /usr/share/doc/mango/
COPY --from=notices-build /THIRD-PARTY-LICENSES.txt /usr/share/doc/mango/
COPY --from=core /mango /usr/local/bin/mango
USER mango
# `mango.toml` is looked for here, so it can live in the volume.
WORKDIR /data
ENV HOME=/data \
    MANGO_DATA=/data \
    MANGO_LISTEN=0.0.0.0:8787 \
    MANGO_HOST_DIR=/opt/mango/host \
    MANGO_JAVA=/opt/java/bin/java \
    MANGO_RUNTIME_VERSION=${SUWAYOMI_VERSION} \
    MANGO_DOCKER=1
VOLUME /data
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --start-interval=2s --retries=3 CMD ["mango", "health"]
ENTRYPOINT ["mango"]

# Some sources open a web view. Its browser is downloaded into the data volume the
# first time one does; this image has the libraries the browser links and the
# program that lends it a display. Mango finds that program and uses it by itself.
FROM base AS full
USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libxss1 libxext6 libxrender1 libxcomposite1 libxdamage1 libxkbcommon0 libxtst6 libxcursor1 libxi6 \
      libglib2.0-0 libnss3 libdbus-1-3 libpango-1.0-0 libcairo2 libasound2 libatk-bridge2.0-0 libcups2 \
      libdrm2 libgbm1 libegl1 xvfb xauth \
 && rm -rf /var/lib/apt/lists/*
USER mango

FROM base AS mango
