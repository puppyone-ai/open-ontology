import fs from "node:fs";
import http from "node:http";
import https from "node:https";

const ca = fs.readFileSync("/secrets/ca.crt");
const agent = new https.Agent({ ca, keepAlive: true, servername: "oo-fe01-proof" });
const edge = https.createServer(
  {
    cert: fs.readFileSync("/secrets/edge.crt"),
    key: fs.readFileSync("/secrets/edge.key"),
  },
  (request, response) => {
    const path = request.url || "/";
    const control = path.startsWith("/control/");
    const upstream = (control ? https : http).request(
      control
        ? {
            host: "oo-fe01-proof",
            port: 7443,
            method: request.method,
            path,
            headers: { ...request.headers, host: "oo-fe01-proof:7443" },
            agent,
            servername: "oo-fe01-proof",
          }
        : {
            host: "127.0.0.1",
            port: 4173,
            method: request.method,
            path,
            headers: { ...request.headers, host: "127.0.0.1:4173" },
          },
      (incoming) => {
        response.writeHead(incoming.statusCode || 502, incoming.headers);
        incoming.pipe(response);
      },
    );
    upstream.on("error", () => {
      if (!response.headersSent) {
        response.writeHead(502);
      }
      response.end();
    });
    request.pipe(upstream);
  },
);
edge.listen(8443, "0.0.0.0");
