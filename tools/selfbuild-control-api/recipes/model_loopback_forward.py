#!/usr/bin/env python3
"""Container loopback transport to the unchanged host product proxy; no logging."""
import socket,threading

def copy(source,target):
    try:
        while True:
            chunk=source.recv(65536)
            if not chunk: break
            target.sendall(chunk)
    finally:
        try: target.shutdown(socket.SHUT_WR)
        except OSError: pass

def handle(client):
    try:
        with client, socket.create_connection(('host.docker.internal',8317),timeout=120) as upstream:
            worker=threading.Thread(target=copy,args=(client,upstream));worker.start();copy(upstream,client);worker.join()
    except OSError: pass

with socket.socket() as server:
    server.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);server.bind(('127.0.0.1',8317));server.listen(32)
    while True:
        client,_=server.accept();threading.Thread(target=handle,args=(client,),daemon=True).start()
