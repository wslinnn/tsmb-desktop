import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import MainWindow from "./windows/main/MainWindow";
import LyricsWindow from "./windows/lyrics/LyricsWindow";
import "./styles.css";

// 两个窗口共用一个 bundle，按窗口 label 分流渲染
const label = getCurrentWebviewWindow().label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {label === "lyrics" ? <LyricsWindow /> : <MainWindow />}
  </React.StrictMode>
);
