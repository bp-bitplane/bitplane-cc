import { useEffect } from "react";
import styles from "./App.module.css";
import { Dialogs } from "./dialogs/Dialogs";
import { MainArea } from "./main-area/MainArea";
import { Sidebar } from "./sidebar/Sidebar";
import { connect } from "./store/store";
import { useShortcuts } from "./useShortcuts";
import { useTitlebarOverlay } from "./useTitlebarOverlay";

export function App() {
  useShortcuts();
  const overlay = useTitlebarOverlay();
  useEffect(() => {
    const connection = connect();
    return () => {
      void connection.then((disconnect) => disconnect());
    };
  }, []);

  return (
    <div className={styles.app} data-titlebar={overlay ? "overlay" : undefined}>
      <Sidebar />
      <MainArea />
      <Dialogs />
    </div>
  );
}
