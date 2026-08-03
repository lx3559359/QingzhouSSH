FROM ubuntu:24.04

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server sudo \
 && mkdir -p /run/sshd \
 && useradd -m -s /bin/bash testuser \
 && install -d -m 700 -o testuser -g testuser /home/testuser/.ssh \
 && echo 'testuser:testpass' | chpasswd \
 && printf '\nPasswordAuthentication yes\nPubkeyAuthentication yes\nStrictModes no\nPermitRootLogin no\n' >> /etc/ssh/sshd_config \
 && rm -rf /var/lib/apt/lists/*

EXPOSE 22

CMD ["/usr/sbin/sshd", "-D", "-e"]
