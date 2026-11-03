#!/bin/sh
# film-sub nginx entrypoint: put the host-gateway IP in place of __GW__, then start nginx
set -e
GW=$(getent hosts host-gateway | awk '{print $1}')
if [ -z "$GW" ]; then GW=host-gateway; fi
echo "[nginx-film-sub] host-gateway = $GW"
sed "s|__GW__|${GW}|g" /etc/nginx-extra/default.conf.tpl > /etc/nginx/conf.d/default.conf
exec nginx -g 'daemon off;'
