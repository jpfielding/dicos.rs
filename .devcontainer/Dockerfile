FROM rockylinux/rockylinux:10 AS base

ARG REPO_NAME=dicos.rs
ARG USERNAME=ferris
ARG USER_UID=1000
ARG USER_GID=$USER_UID

RUN set -eux -o pipefail && \
    dnf install -y epel-release && \
    dnf -y update
RUN set -eux -o pipefail && \
    dnf install -y ca-certificates curl-minimal git git-lfs iproute jq make msitools ncurses nss-tools procps sudo wget which zsh zip && \
    dnf install -y mesa-libGL-devel libX11-devel libXrandr-devel libXxf86dga libXxf86vm libXi-devel libXcursor-devel libXinerama-devel

FROM base as user

# Create the user
RUN groupadd --gid ${USER_GID} ${USERNAME} && \
  useradd --uid ${USER_UID} --gid ${USER_GID} -m ${USERNAME} -s /bin/zsh && \
  echo ${USERNAME} ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/${USERNAME} && \
  chmod 0440 /etc/sudoers.d/${USERNAME}

########### NON ROOT FROM HERE DOWN ###########
USER ${USERNAME}

RUN set -eux && \
    mkdir -p ${HOME}/bin && \
    sudo mkdir -p /workdir/${REPO_NAME} && \
    sudo chown ${USERNAME}:${USERNAME} /workdir

# envs
ENV HOME "/home/${USERNAME}"
ENV SDK_HOME ${HOME}/sdk
ENV PATH ${HOME}/bin:${PATH}
RUN mkdir -p ${SDK_HOME}

# rust — keep RUST_VERSION in sync with /rust-toolchain.toml (single source of truth at runtime)
ARG RUST_VERSION=1.95.0
ENV PATH ${HOME}/.cargo/bin:${PATH}
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
        bash -s -- -y --default-toolchain ${RUST_VERSION} --profile minimal -c rustfmt,clippy

# pretty bash
COPY --chown=${USERNAME}:${USERNAME} .bash_pretty ${HOME}/.bash_rust
RUN set -eux && \
    echo "source ${HOME}/.bash_rust" >> ${HOME}/.bashrc

# pretty bash
COPY --chown=${USERNAME}:${USERNAME} .bash_pretty ${HOME}/.bash_pretty
RUN set -eux && \
    echo "source ${HOME}/.bash_pretty" >> ${HOME}/.bashrc

# go
ENV GO_VERSION "1.26.3"
ENV GOPATH ${SDK_HOME}/go
ENV PATH ${GOPATH}/bin:${HOME}/sdk/go:${PATH}
RUN set -eux && \
    export GO_ARCH="$(arch | sed 's/aarch64/arm64/' | sed 's/x86_64/amd64/')" && \
    cd /tmp && mkdir -p ${SDK_HOME} && \
    curl -k -o go.tar.gz -L https://go.dev/dl/go${GO_VERSION}.linux-${GO_ARCH}.tar.gz && \
    tar xzf go.tar.gz && mv go ${SDK_HOME} && rm /tmp/go.tar.gz && \
    cd ${SDK_HOME} && mv go go${GO_VERSION} && ln -s go${GO_VERSION} go


