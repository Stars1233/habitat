FROM alpine
ARG HAB_VERSION=
ARG PACKAGE_TARGET
RUN --mount=type=secret,id=hab_auth_token \
  set -ex \
  && apk add --no-cache --virtual .build-deps \
    ca-certificates \
    gnupg \
    wget \
    bash \
  \
  && cd /tmp \
  && wget https://raw.githubusercontent.com/habitat-sh/habitat/main/components/hab/install.sh \
  && export HAB_AUTH_TOKEN=$(cat /run/secrets/hab_auth_token) \
  && bash install.sh -t ${PACKAGE_TARGET} -c acceptance -o chef \
  && unset HAB_AUTH_TOKEN \
  && rm -rf install.sh /hab/cache /root/.wget-hsts /root/.gnupg \
  && apk del .build-deps
