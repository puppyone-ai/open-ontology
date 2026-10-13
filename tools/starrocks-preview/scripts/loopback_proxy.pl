#!/usr/bin/perl
# Forward 127.0.0.1:8317 to host.docker.internal:8317.
# pg-builder accepts only a loopback origin. Docker Desktop does not publish
# the Mac loopback listener at the container's 127.0.0.1. No request bytes are logged.
use strict;
use warnings;
use IO::Select;
use IO::Socket::IP;

my $listen = IO::Socket::IP->new(
    LocalAddr => "127.0.0.1",
    LocalPort => 8317,
    Listen => 16,
    ReuseAddr => 1,
) or die "listen: $!\n";
$SIG{CHLD} = "IGNORE";
while (my $client = $listen->accept) {
    my $pid = fork();
    next if !defined $pid || $pid;
    my $remote = IO::Socket::IP->new(
        PeerHost => "host.docker.internal",
        PeerPort => 8317,
        Timeout => 30,
    );
    exit 1 if !$remote;
    $client->blocking(1);
    $remote->blocking(1);
    $client->autoflush(1);
    $remote->autoflush(1);
    my $select = IO::Select->new($client, $remote);
    while (my @ready = $select->can_read) {
        for my $handle (@ready) {
            my $other = $handle == $client ? $remote : $client;
            my $read = sysread($handle, my $buffer, 65536);
            if (!defined $read || $read == 0) {
                exit 0;
            }
            my $offset = 0;
            while ($offset < $read) {
                my $wrote = syswrite($other, $buffer, $read - $offset, $offset);
                exit 0 if !defined $wrote || $wrote == 0;
                $offset += $wrote;
            }
        }
    }
    exit 0;
}
