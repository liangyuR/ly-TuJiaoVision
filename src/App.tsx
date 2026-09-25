import { Navigate, Route, Routes } from "react-router-dom";
import AppLayout from "./layout/AppLayout";
import { navItems } from "./layout/nav";

export default function App() {
  return (
    <Routes>
      <Route element={<AppLayout />}>
        <Route index element={<Navigate to={navItems[0].path} replace />} />
        {navItems.map(({ path, element: Page }) => (
          <Route key={path} path={path} element={<Page />} />
        ))}
        <Route path="*" element={<Navigate to={navItems[0].path} replace />} />
      </Route>
    </Routes>
  );
}
