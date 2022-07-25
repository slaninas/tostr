FROM rust:1.62-bullseye
# TODO: Specific commits for used repos, don't just use master HEAD

# Prevent being stuck at timezone selection
ENV TZ=Europe/London
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone


RUN apt update && apt install -y gpg wget vim git g++ python3 python3-pip expect-dev

# Build twint
# Also prevent crash when profile doesn't have url or banner url
RUN git clone --depth=1 https://github.com/minamotorin/twint.git && \
    cd twint && \
    pip3 install . -r requirements.txt && \
    sed -i 's/    _usr\.url =.*/    _usr\.url = ""/'  /usr/local/lib/python3.9/dist-packages/twint/user.py  && \
    sed -i 's/    _usr\.background_image =.*/    _usr\.background_image = ""/'  /usr/local/lib/python3.9/dist-packages/twint/user.py



ARG CODENAME=bullseye
# Setup tor https://support.torproject.org/apt/tor-deb-repo/
RUN apt install -y apt-transport-https
RUN echo "deb [signed-by=/usr/share/keyrings/tor-archive-keyring.gpg] https://deb.torproject.org/torproject.org ${CODENAME} main\ndeb-src [signed-by=/usr/share/keyrings/tor-archive-keyring.gpg] https://deb.torproject.org/torproject.org ${CODENAME} main" > /etc/apt/sources.list.d/tor.list
RUN wget -qO- https://deb.torproject.org/torproject.org/A3C4F0F979CAA22CDBA8F512EE8CBC9E886DDD89.asc | gpg --dearmor | tee /usr/share/keyrings/tor-archive-keyring.gpg >/dev/null
RUN apt update && apt install -y tor deb.torproject.org-keyring

RUN echo "HiddenServiceDir /var/lib/tor/hidden_service/\nHiddenServicePort 8080 127.0.0.1:8080" >> /etc/tor/torrc && \
    mkdir /var/lib/tor/hidden_service/ && \
    chown debian-tor:debian-tor /var/lib/tor/hidden_service/ && \
    chmod 0700 /var/lib/tor/hidden_service/

COPY Cargo.toml /app/
COPY src /app/src

RUN cd /app && \
    cargo build --release

# TODO: Add non-root user and use it
COPY config /app/
ENV RUST_LOG=debug

# Use unbuffer to preserve colors in terminal output while using tee
CMD cd /app && unbuffer cargo run --release 2>&1 | tee -a data/log
