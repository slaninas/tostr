FROM rust:1.61

# Prevent being stuck at timezone selection
ENV TZ=Europe/London
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone


RUN apt update && apt install -y gpg wget vim git g++ python3 python3-pip

# Build twint
RUN git clone --depth=1 https://github.com/minamotorin/twint.git && \
    cd twint && \
    pip3 install . -r requirements.txt

# Build nostril
RUN git clone https://github.com/bitcoin-core/secp256k1 && \
    cd secp256k1 && \
    ./autogen.sh && \
    ./configure --enable-module-ecdh --enable-module-schnorrsig && \
    make install
RUN git clone https://github.com/jb55/nostril && \
    cd nostril && \
    make

# Build websocat
RUN git clone https://github.com/vi/websocat.git && \
    cd websocat && \
    cargo install --features=ssl websocat

ENV LD_LIBRARY_PATH=/usr/local/lib
WORKDIR /app
RUN bash -c 'cd /app'
