FROM alpine:3.22
LABEL oo.lane="b"
RUN apk add --no-cache openssh python3
COPY mysql_fault_proxy.py /mysql_fault_proxy.py
CMD ["sh", "-c", "cp /run/private/host-key /etc/ssh/ssh_host_ed25519_key; chmod 600 /etc/ssh/ssh_host_ed25519_key; mkdir -p /root/.ssh; cp /run/private/client-key.pub /root/.ssh/authorized_keys; chmod 700 /root/.ssh; chmod 600 /root/.ssh/authorized_keys; sed 's/^/root:/' /run/private/jump-password | chpasswd; if [ \"$OO_MYSQL_PROXY\" = 1 ]; then python3 /mysql_fault_proxy.py & fi; exec /usr/sbin/sshd -D -e -f /run/private/sshd_config"]
