import { Navigate, Route, Routes } from "react-router-dom";
import AppLayout from "./layout/AppLayout";
import { navItems } from "./layout/nav";
import HistoryDetailPage from "./pages/HistoryDetailPage";

export default function App() {
  return (
    <Routes>
      <Route element={<AppLayout />}>
        <Route index element={<Navigate to={navItems[0].path} replace />} />
        {navItems.map(({ path, element: Page }) => (
          <Route key={path} path={path} element={<Page />} />
        ))}
        <Route path="/history/:id" element={<HistoryDetailPage />} />
        <Route path="*" element={<Navigate to={navItems[0].path} replace />} />
      </Route>
    </Routes>
  );
}
