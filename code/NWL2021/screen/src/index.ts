require("dotenv").config();

import * as path from "path";
import express from "express";
import compression from "compression";
import http from "http";
import config from "../../config";
import { io } from "socket.io-client";
import { Server } from "socket.io";

export default async function start() {
  const app = express();
  app.use(express.json());

  app.use(compression());
  app.set("trust proxy", 1);
  app.set("etag", "strong");

  app.use(
    express.static(path.join(__dirname, "..", "public"), {
      etag: true,
      lastModified: true,
      maxAge: 0, // 1h
    })
  );

  const server = http.createServer(app);
  const serverIo = new Server(server);

  const socket = io(config.SERVER_HOST + "screen");

  let setup = null;

  serverIo.on("connection", (socket) => {
    console.log("Screen connected");
    if (setup) socket.emit("setup", setup);
    socket.on("disconnect", function () {
      console.log("Screen disconnected");
    });
  });

  socket.on("setup", function (data) {
    setup = data;
    serverIo.emit("setup", setup);
  });

  socket.on("gameStateUpdate", (data) => {
    serverIo.emit("gameStateUpdate", data);
  });

  server.listen(config.SCREEN_PORT);
  console.log(
    "Screen server started on port: " + config.SCREEN_PORT
  );
}

start();