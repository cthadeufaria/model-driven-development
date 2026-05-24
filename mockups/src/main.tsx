import React from 'react'
import ReactDOM from 'react-dom/client'
import { createBrowserRouter, RouterProvider, Link } from 'react-router-dom'
import { ViewerFull } from './mockups/viewer-full'
import './styles.css'

function Index() {
  return (
    <div className="index">
      <h1>mdd mockups</h1>
      <p>High-fidelity React renders paired with the Salt contracts under <code>.mdd/models/**/mockups/</code>.</p>
      <ul>
        <li>
          <Link to="/mockup/viewer-full">/mockup/viewer-full</Link>
          {' '}— MCK-VIEWER-FULL (full viewer window)
        </li>
      </ul>
    </div>
  )
}

const router = createBrowserRouter([
  { path: '/', element: <Index /> },
  { path: '/mockup/viewer-full', element: <ViewerFull /> },
])

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>,
)
