import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import MainWindow from "./windows/main/MainWindow";
import LyricsWindow from "./windows/lyrics/LyricsWindow";
import "./styles.css";

// 两个窗口共用一个 bundle，按窗口 label 分流渲染
const label = getCurrentWebviewWindow().label;

// 禁用 WebView 默认右键菜单：桌面歌词锁按钮等位置右键属误触，
// 弹出的浏览器菜单（刷新/检查等）对终端用户无意义
document.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {label === "lyrics" ? <LyricsWindow /> : <MainWindow />}
  </React.StrictMode>
);
