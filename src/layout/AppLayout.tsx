import { Outlet, useLocation } from "react-router-dom";
import Sidebar from "./Sidebar";
import { navItems } from "./nav";

export default function AppLayout() {
  const { pathname } = useLocation();
  const current = navItems.find((item) => pathname === item.path || pathname.startsWith(`${item.path}/`));

  return (
    <div className="app-shell">
      <Sidebar />
      <main className="main">
        <header className="main-header">
          <h1>{current?.label ?? ""}</h1>
        </header>
        <section className="main-body">
          <Outlet />
        </section>
      </main>
    </div>
  );
}
