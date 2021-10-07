import * as fs from "fs";
import * as path from "path";
import * as express from "express";
import LaureateModel from "../database/laureates/laureates.model";
import QuestionModel from "../database/questions/questions.model";
import StateModel from "../database/state/state.model";
import { importDefaultConfiguration } from "../../import";
import { Engine } from "../engine";

const router = express.Router();

let engine: Engine = null;
export default function (e) {
  engine = e;
  return router;
}

router.post("/laureate", (req, res) => {
  const data = req.body;
  new LaureateModel(data).save();
});

router.post("/state/load", async (req, res) => {
  await importDefaultConfiguration();
  engine.state = await StateModel.findOne();
  res.json("ok");
});

function cleanDbObject(data: any) {
  if (Array.isArray(data)) {
    for (const e of data) {
      cleanDbObject(e);
    }
  } else if (typeof data == "object") {
    for (const i in data) {
      if (i == "__v") {
        delete data[i];
      } else {
        cleanDbObject(data[i]);
      }
    }
  }
  return data;
}
router.post("/state/export", async (req, res) => {
  const gameState = (await StateModel.findOne()).toJSON();
  const questions = (await QuestionModel.find()).map((doc) => doc.toJSON());
  const laureates = (await LaureateModel.find()).map((doc) => doc.toJSON());
  await fs.promises.writeFile(
    path.join(__dirname, "../../data/state.json"),
    JSON.stringify(cleanDbObject(gameState), null, 2)
  );

  await fs.promises.writeFile(
    path.join(__dirname, "../../data/questions.json"),
    JSON.stringify(cleanDbObject(questions), null, 2)
  );

  await fs.promises.writeFile(
    path.join(__dirname, "../../data/laureates.json"),
    JSON.stringify(cleanDbObject(laureates), null, 2)
  );
  return res.json("ok");
});

router.get("/questions", async (req, res) => {
  res.json(
    cleanDbObject((await QuestionModel.find()).map((doc) => doc.toJSON()))
  );
});

router.post("/laureates", async (req, res) => {
  const laureates = req.body;
  for (const laureate of laureates) {
    if (laureate.firstname == null) continue;
    let q = new LaureateModel(laureate);
    if (laureate._id) {
      q.isNew = false;
    }
    await q.save();
  }
  return res.json("ok");
});

router.post("/questions", async (req, res) => {
  const questions = req.body;
  for (const question of questions) {
    if (question.text == null) continue;
    let q = new QuestionModel(question);
    if (question._id) {
      q.isNew = false;
    }
    await q.save();
  }
  return res.json("ok");
});

router.get("/state", async (req, res) => {
  const gameState = (await StateModel.findOne())?.toJSON();
  res.json(cleanDbObject(gameState));
});

router.post("/state", async (req, res) => {
  const gameState = await StateModel.findOne();
  gameState.width = req.body.width;
  gameState.height = req.body.height;
  gameState.unitSize = req.body.unitSize;
  await gameState.save();
  engine.state = await StateModel.findOne();
  res.json("ok");
});