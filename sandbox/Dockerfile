FROM docker.io/library/python:3.12-slim

# iptables is run only by the root, NET_ADMIN-holding container that puts the
# networked sandbox's egress rules in place (server/sandbox.js EGRESS_RULES);
# the sandbox itself runs unprivileged with every capability dropped.
RUN pip install --no-cache-dir pillow numpy \
    && apt-get update \
    && apt-get install -y --no-install-recommends jq ripgrep imagemagick poppler-utils curl ca-certificates file librsvg2-bin iptables \
    && rm -rf /var/lib/apt/lists/*

# The launcher runs as the host UID/GID so /work stays writable on both sides.
WORKDIR /work
CMD ["sleep", "infinity"]
