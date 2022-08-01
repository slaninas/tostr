FROM rust:1.62-bullseye
ARG CODENAME=bullseye
# TODO: Specific commits for used repos, don't just use master HEAD

# Prevent being stuck at timezone selection
ENV TZ=Europe/London
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone


RUN apt update && apt install -y gpg wget vim git g++ python3 python3-pip expect-dev

# Setup tor https://support.torproject.org/apt/tor-deb-repo/
RUN apt install -y apt-transport-https iptables
RUN echo "deb [signed-by=/usr/share/keyrings/tor-archive-keyring.gpg] https://deb.torproject.org/torproject.org ${CODENAME} main\ndeb-src [signed-by=/usr/share/keyrings/tor-archive-keyring.gpg] https://deb.torproject.org/torproject.org ${CODENAME} main" > /etc/apt/sources.list.d/tor.list
RUN wget -qO- https://deb.torproject.org/torproject.org/A3C4F0F979CAA22CDBA8F512EE8CBC9E886DDD89.asc | gpg --dearmor | tee /usr/share/keyrings/tor-archive-keyring.gpg >/dev/null
RUN apt update && apt install -y tor deb.torproject.org-keyring


ARG NETWORK

# Build twint
# Also prevent crash when profile doesn't have url or banner url and make twint use tor proxy
RUN git clone --depth=1 https://github.com/minamotorin/twint.git && \
    cd twint && \
    sed -i 's/    _usr\.url =.*/    _usr\.url = ""/'  twint/user.py  && \
    sed -i 's/    _usr\.background_image =.*/    _usr\.background_image = ""/'  twint/user.py && \
    if [ "$NETWORK" = "tor" ]; then sed -i "s/async with aiohttp.ClientSession(connector.*/async with aiohttp.ClientSession(connector=ProxyConnector(host='127.0.0.1', port='9050', rdns=True), headers=headers) as session:/" twint/get.py; fi && \
    if [ "$NETWORK" = "tor" ]; then sed -i "s/r = self._session.send(req, allow_redirects=True, timeout=self._timeout.*/r = self._session.send(req, allow_redirects=True, timeout=self._timeout, proxies={'https': 'socks5h:\/\/127.0.0.1:9050'})/" twint/token.py; fi && \
    pip3 install . -r requirements.txt

COPY config /app/
COPY startup_clearnet.sh startup_tor.sh /
RUN if [ "$NETWORK" = "clearnet" ]; then ln -s /startup_clearnet.sh /startup.sh; elif [ "$NETWORK" = "tor" ]; then ln -s /startup_tor.sh /startup.sh; else exit 1; fi

# TODO: Add non-root user and use it
ENV RUST_LOG=debug

COPY Cargo.toml /app/
COPY src /app/src

RUN cd app && cargo build --release


# Use unbuffer to preserve colors in terminal output while using tee
CMD cd /app && unbuffer /startup.sh
