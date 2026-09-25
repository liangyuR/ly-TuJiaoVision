import type { ComponentType } from "react";
import { Camera, History, ScanEye, Settings, SlidersHorizontal, type LucideIcon } from "lucide-react";
import InspectPage from "../pages/InspectPage";
import CameraPage from "../pages/CameraPage";
import RecipePage from "../pages/RecipePage";
import HistoryPage from "../pages/HistoryPage";
import SettingsPage from "../pages/SettingsPage";

export interface NavItem {
  path: string;
  label: string;
  icon: LucideIcon;
  element: ComponentType;
}

export const navItems: NavItem[] = [
  { path: "/inspect", label: "实时检测", icon: ScanEye, element: InspectPage },
  { path: "/camera", label: "图像源", icon: Camera, element: CameraPage },
  { path: "/recipe", label: "检测配方", icon: SlidersHorizontal, element: RecipePage },
  { path: "/history", label: "历史记录", icon: History, element: HistoryPage },
  { path: "/settings", label: "系统设置", icon: Settings, element: SettingsPage },
];
