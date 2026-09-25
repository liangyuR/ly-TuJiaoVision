import { FolderOpen, RefreshCw } from "lucide-react";

export default function CameraPage() {
  return (
    <div className="stack">
      <div className="panel">
        <div className="panel-toolbar">
          <h3 className="panel-title">设备列表</h3>
          <button className="btn">
            <RefreshCw size={16} />
            刷新
          </button>
        </div>
        <div className="empty small">
          <p>未发现相机设备</p>
        </div>
      </div>
      <div className="panel">
        <div className="panel-toolbar">
          <h3 className="panel-title">离线图片</h3>
          <button className="btn">
            <FolderOpen size={16} />
            选择目录
          </button>
        </div>
        <div className="empty small">
          <p>可从本地目录加载图片用于调试</p>
        </div>
      </div>
    </div>
  );
}
