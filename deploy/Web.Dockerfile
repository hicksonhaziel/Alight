FROM node:22.22.0-bookworm-slim AS web
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --ignore-scripts --no-audit --no-fund
COPY web ./
COPY sdk/ts/src ../sdk/ts/src
RUN npm run build

FROM caddy:2.10.2-alpine
COPY --from=web /build/web/dist /srv/alight
COPY deploy/Caddyfile /etc/caddy/Caddyfile
