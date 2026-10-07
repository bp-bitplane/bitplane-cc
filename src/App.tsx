import { useEffect } from "react";
import styles from "./App.module.css";
import { Dialogs } from "./dialogs/Dialogs";
import { MainArea } from "./main-area/MainArea";
import { Sidebar } from "./sidebar/Sidebar";
import { connect } from "./store/store";
import { useShortcuts } from "./useShortcuts";

export function App() {
  useShortcuts();
  useEffect(() => {
    const connection = connect();
    return () => {
      void connection.then((disconnect) => disconnect());
    };
  }, []);

  return (
    <div className={styles.app}>
      <Sidebar />
      <MainArea />
      <Dialogs />
    </div>
  );
}
