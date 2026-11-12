# check=skip=SecretsUsedInArgOrEnv
# itero on Debian, with what itero's examples use beside it: curl, jq,
# python3, git, ImageMagick and Chromium. The build context holds a static
# musl itero per architecture, bin/<arch>/itero, and its licences: the
# release's own binary (release.yml), or scripts/image.sh's build.
FROM debian:13-slim
ARG TARGETARCH
RUN apt-get -o Acquire::Retries=3 update \
 && apt-get -o Acquire::Retries=3 install -y --no-install-recommends \
      tini ca-certificates curl jq python3 git imagemagick chromium \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --gid 10001 itero \
 && useradd --uid 10001 --gid 10001 --home-dir /var/lib/itero --no-create-home \
      --shell /usr/sbin/nologin itero \
 && mkdir -p /var/lib/itero /etc/itero \
 && chown itero:itero /var/lib/itero \
 && echo '{}' > /etc/itero/daemon.json
COPY bin/${TARGETARCH}/itero /usr/local/bin/itero
COPY bin/${TARGETARCH}/THIRD-PARTY-LICENSES /usr/share/doc/itero/THIRD-PARTY-LICENSES
# (the check skipped on the first line takes ITERO_NO_SECRET_STORE for a
# secret by its name; it only says there is no store)
# The state is /var/lib/itero; no OS secret store in a container
# (secrets: the secrets folder, the environment); Chrome as this user
# cannot use its sandbox. No XDG_CONFIG_HOME: the programs actions run
# keep their settings in ~/.config, on the volume, as /etc is root's.
ENV XDG_STATE_HOME=/var/lib \
    HOME=/var/lib/itero \
    ITERO_NO_SECRET_STORE=1 \
    ITERO_BROWSER_SANDBOX=off
LABEL org.opencontainers.image.source="https://github.com/aegea-dev/itero" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.description="itero: workflows from anything, for anything"
USER 10001:10001
WORKDIR /var/lib/itero
VOLUME /var/lib/itero
EXPOSE 8443
# tini forwards SIGTERM and reaps what the steps leave behind. The
# daemon's settings: /etc/itero/daemon.json, {} (local only) unless one is
# mounted over it, as Compose and Kubernetes do.
ENTRYPOINT ["tini", "--", "itero"]
CMD ["daemon", "--config", "/etc/itero/daemon.json"]
