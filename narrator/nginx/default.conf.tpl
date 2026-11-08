# film-sub nginx: one port (8080) for every service.
# __GW__ = host-gateway IP (filled in by entrypoint.sh), to reach the two VieNeu instances on host ports.

upstream vieneu_pool {
    zone vieneu_pool 64k;
    server __GW__:7861 max_fails=3 fail_timeout=30s;
    server __GW__:7862 max_fails=3 fail_timeout=30s;
}

# Sticky: the first request is round-robin; nginx sets a vieneu_node cookie; later requests keep that instance.
map $cookie_vieneu_node $vieneu_target {
    "1"     __GW__:7861;
    "2"     __GW__:7862;
    default vieneu_pool;
}

map $upstream_addr $vieneu_node_name {
    __GW__:7861 "1";
    __GW__:7862 "2";
    default     "";
}

# Docker's DNS, asked again every few seconds for the services that are set below through a variable: a container that is
# recreated (an update) gets a new IP, and nginx must follow it without a reload (a name in a plain proxy_pass is looked up
# once, at start). Names given by extra_hosts (easyocr on another machine) are in /etc/hosts, not in this DNS: those stay plain.
resolver 127.0.0.11 valid=5s ipv6=off;

server {
    listen 80;
    client_max_body_size 2048m;

    location = / {
        default_type text/plain;
        return 200 "film-sub OK: /whisper/ /translate/ /ocr/ /tts/\n";
    }

    # faster-whisper: POST /whisper/v1/transcribe, GET /whisper/health
    location /whisper/ {
        set $whisper http://whisper:9000;
        rewrite ^/whisper/(.*)$ /$1 break;
        proxy_pass $whisper;
        proxy_read_timeout 14400s;
        proxy_send_timeout 14400s;
        proxy_request_buffering off;
    }

    # LibreTranslate/argos: GET /translate/languages, POST /translate/translate
    location /translate/ {
        set $argos http://argos:5000;
        rewrite ^/translate/(.*)$ /$1 break;
        proxy_pass $argos;
        proxy_read_timeout 300s;
    }

    # PaddleOCR: POST /ocr/ocr (image), GET /ocr/health
    location /ocr/ {
        proxy_pass http://easyocr:8888/;
        proxy_read_timeout 1800s;
        client_max_body_size 200m;
    }

    # VieNeu TTS API (clients use the base URL http://IP:8080; Gradio builds file URLs from the Host header,
    # so the original host is passed through)
    location /gradio_api/ {
        proxy_pass http://$vieneu_target;
        proxy_set_header Host $http_host;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 7200s;
        proxy_send_timeout 7200s;
        proxy_buffering off;
        add_header Set-Cookie "vieneu_node=$vieneu_node_name; Path=/" always;
    }

    # VieNeu web UI (in a browser)
    location /tts/ {
        rewrite ^/tts/(.*)$ /$1 break;
        proxy_pass http://$vieneu_target;
        proxy_set_header Host $http_host;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 7200s;
        proxy_send_timeout 7200s;
        proxy_buffering off;
        # sticky cookie for the instance that answered
        add_header Set-Cookie "vieneu_node=$vieneu_node_name; Path=/" always;
    }
}
